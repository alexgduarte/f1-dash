//! A minimal client for classic ASP.NET SignalR (2.x, protocol 1.5) over
//! WebSockets, as used by the FIA Formula 2 / Formula 3 / F1 Academy timing
//! service.
//!
//! Flow: `GET {base}/negotiate` for a connection token (keeping its cookies),
//! open `wss://{base}/connect`, wait for the init frame, `GET {base}/start`,
//! then send hub invocations `{"H","M","A","I"}`. The server answers with
//! results `{"R","I"}` / errors `{"E","I"}` and pushes hub calls in
//! `{"C","M":[{"H","M","A"}]}` frames; `{}` frames are keep-alives.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Error, anyhow};
use futures::{SinkExt, Stream, StreamExt};
use reqwest::{
    Url,
    cookie::{CookieStore, Jar},
    header::{self, HeaderValue},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use tracing::{debug, warn};

const CLIENT_PROTOCOL: &str = "1.5";
const INIT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct ClassicOptions<'a> {
    /// e.g. `https://ltss.fiaformula2.com/streaming`
    pub base_url: &'a str,
    pub hub: &'a str,
    /// Sent as `Origin`; some deployments only accept their own site.
    pub origin: Option<&'a str>,
    pub user_agent: &'a str,
}

#[derive(Deserialize)]
struct Negotiation {
    #[serde(rename = "ConnectionToken")]
    connection_token: String,
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct ClassicClient {
    socket: Socket,
    hub: String,
    next_id: u64,
}

/// A frame from the server, split into what callers care about.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassicMessage {
    /// The result of an invocation sent with [`ClassicClient::invoke`].
    Result {
        id: String,
        result: Option<Value>,
        error: Option<String>,
    },
    /// A hub method called by the server.
    Invocation { method: String, args: Vec<Value> },
}

fn connection_data(hub: &str) -> String {
    json!([{ "name": hub }]).to_string()
}

fn transport_url(base: &str, path: &str, token: &str, hub: &str) -> Result<Url, Error> {
    let mut url = Url::parse(&format!("{}/{path}", base.trim_end_matches('/')))?;
    url.query_pairs_mut()
        .append_pair("transport", "webSockets")
        .append_pair("clientProtocol", CLIENT_PROTOCOL)
        .append_pair("connectionToken", token)
        .append_pair("connectionData", &connection_data(hub));
    Ok(url)
}

pub async fn connect(options: ClassicOptions<'_>) -> Result<ClassicClient, Error> {
    crate::ensure_crypto_provider();

    let jar = Arc::new(Jar::default());

    let mut headers = header::HeaderMap::new();
    if let Some(origin) = options.origin {
        headers.insert(header::ORIGIN, HeaderValue::from_str(origin)?);
    }

    let http = reqwest::Client::builder()
        .cookie_provider(jar.clone())
        .user_agent(options.user_agent)
        .default_headers(headers)
        .timeout(Duration::from_secs(15))
        .build()?;

    let base = options.base_url.trim_end_matches('/');

    let mut negotiate_url = Url::parse(&format!("{base}/negotiate"))?;
    negotiate_url
        .query_pairs_mut()
        .append_pair("clientProtocol", CLIENT_PROTOCOL)
        .append_pair("connectionData", &connection_data(options.hub));

    let negotiation: Negotiation = http
        .get(negotiate_url.clone())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("invalid negotiate response")?;

    let mut ws_url = transport_url(base, "connect", &negotiation.connection_token, options.hub)?;
    ws_url.query_pairs_mut().append_pair("tid", "7");
    let scheme = if ws_url.scheme() == "http" {
        "ws"
    } else {
        "wss"
    };
    ws_url
        .set_scheme(scheme)
        .map_err(|_| anyhow!("cannot use {scheme} for {ws_url}"))?;

    let mut request = ws_url.as_str().into_client_request()?;
    let request_headers = request.headers_mut();
    request_headers.insert(
        header::USER_AGENT,
        HeaderValue::from_str(options.user_agent)?,
    );
    if let Some(origin) = options.origin {
        request_headers.insert(header::ORIGIN, HeaderValue::from_str(origin)?);
    }
    // load balancers pin the socket to the node that issued the token
    if let Some(cookies) = jar.cookies(&negotiate_url) {
        request_headers.insert(header::COOKIE, cookies);
    }

    let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;

    // the server confirms the connection with an init frame ({"S":1,...})
    let init = tokio::time::timeout(INIT_TIMEOUT, socket.next())
        .await
        .context("no init frame from server")?
        .context("connection closed before init")??;
    debug!(?init, "classic signalr connected");

    let start_url = transport_url(base, "start", &negotiation.connection_token, options.hub)?;
    match http.get(start_url).send().await {
        Ok(res) if res.status().is_success() => {}
        Ok(res) => warn!(status = %res.status(), "signalr start request failed"),
        Err(err) => warn!(?err, "signalr start request failed"),
    }

    Ok(ClassicClient {
        socket,
        hub: options.hub.to_owned(),
        next_id: 0,
    })
}

impl ClassicClient {
    /// Calls a hub method and returns the invocation id its result carries.
    pub async fn invoke(&mut self, method: &str, args: Vec<Value>) -> Result<String, Error> {
        let id = self.next_id.to_string();
        self.next_id += 1;

        let message = json!({ "H": self.hub, "M": method, "A": args, "I": id });
        self.socket.send(Message::text(message.to_string())).await?;

        Ok(id)
    }

    /// Server messages until the connection closes.
    pub fn into_stream(self) -> impl Stream<Item = ClassicMessage> + Send + 'static {
        self.socket
            .filter_map(|frame| async move {
                match frame {
                    Ok(Message::Text(text)) => Some(parse_frame(&text)),
                    Ok(Message::Close(_)) => None,
                    Ok(_) => Some(vec![]),
                    Err(err) => {
                        warn!(?err, "classic signalr socket error");
                        None
                    }
                }
            })
            .flat_map(futures::stream::iter)
    }
}

/// Parses one text frame into zero or more messages.
pub fn parse_frame(text: &str) -> Vec<ClassicMessage> {
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        debug!(frame = text, "skipping unparsable frame");
        return vec![];
    };

    let mut messages = Vec::new();

    if let Some(id) = frame.get("I") {
        let id = match id {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };

        messages.push(ClassicMessage::Result {
            id,
            result: frame.get("R").cloned(),
            error: frame.get("E").and_then(Value::as_str).map(str::to_owned),
        });
    }

    for call in frame
        .get("M")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(method) = call.get("M").and_then(Value::as_str) else {
            continue;
        };

        let args = call
            .get("A")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        messages.push(ClassicMessage::Invocation {
            method: method.to_owned(),
            args,
        });
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hub_calls() {
        let frame = r#"{"C":"d-1,2","M":[{"H":"Streaming","M":"trackfeed","A":["ts",{"Value":"2","Message":"Yellow"}]}]}"#;

        assert_eq!(
            parse_frame(frame),
            vec![ClassicMessage::Invocation {
                method: "trackfeed".into(),
                args: vec![json!("ts"), json!({"Value": "2", "Message": "Yellow"})],
            }]
        );
    }

    #[test]
    fn parses_results_and_errors() {
        assert_eq!(
            parse_frame(r#"{"R":{"data":[1,2]},"I":"1"}"#),
            vec![ClassicMessage::Result {
                id: "1".into(),
                result: Some(json!({"data": [1, 2]})),
                error: None
            }]
        );

        assert_eq!(
            parse_frame(r#"{"I":"0","E":"nope"}"#),
            vec![ClassicMessage::Result {
                id: "0".into(),
                result: None,
                error: Some("nope".into())
            }]
        );
    }

    #[test]
    fn ignores_keep_alives_and_init() {
        assert!(parse_frame("{}").is_empty());
        assert!(parse_frame(r#"{"C":"s-0,1","S":1,"M":[]}"#).is_empty());
    }

    #[test]
    fn builds_transport_urls() {
        let url = transport_url(
            "https://example.com/streaming/",
            "connect",
            "a+b",
            "streaming",
        )
        .unwrap();
        assert_eq!(url.path(), "/streaming/connect");
        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert!(query.contains(&("connectionToken".into(), "a+b".into())));
        assert!(query.contains(&("connectionData".into(), r#"[{"name":"streaming"}]"#.into())));
    }
}
