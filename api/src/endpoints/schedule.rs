use axum::{Json, extract::Query, http::StatusCode};
use chrono::Datelike;
use feeds::{
    Series,
    schedule::{self, Round},
};
use serde::Deserialize;
use tracing::error;

#[derive(Debug, Deserialize)]
pub struct ScheduleQuery {
    series: Option<String>,
    year: Option<i32>,
}

impl ScheduleQuery {
    fn resolve(&self) -> Result<(Series, i32), StatusCode> {
        let series = match self.series.as_deref() {
            None | Some("") => Series::F1,
            Some(id) => id.parse().map_err(|_| StatusCode::BAD_REQUEST)?,
        };

        Ok((
            series,
            self.year.unwrap_or_else(|| chrono::Utc::now().year()),
        ))
    }
}

pub async fn get(Query(query): Query<ScheduleQuery>) -> Result<Json<Vec<Round>>, StatusCode> {
    let (series, year) = query.resolve()?;

    schedule::schedule(series, year)
        .await
        .map(Json)
        .map_err(|err| {
            error!(?err, %series, year, "failed to build schedule");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

pub async fn get_next(Query(query): Query<ScheduleQuery>) -> Result<Json<Round>, StatusCode> {
    let (series, year) = query.resolve()?;

    match schedule::next(series, year).await {
        Ok(Some(round)) => Ok(Json(round)),
        Ok(None) => Err(StatusCode::NO_CONTENT),
        Err(err) => {
            error!(?err, %series, year, "failed to build schedule");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
