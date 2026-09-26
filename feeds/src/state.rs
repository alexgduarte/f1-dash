use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The merged JSON state of one live timing feed.
///
/// Every feed adapter normalises its series into the F1 topic layout
/// (`DriverList`, `TimingData`, `TimingAppData`, ...), so the state is kept as
/// plain JSON and partial updates are applied with [`merge`].
#[derive(Clone, Default)]
pub struct StateService {
    state: Arc<RwLock<Value>>,
}

impl StateService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(Value::Object(serde_json::Map::new()))),
        }
    }

    pub async fn get_state(&self) -> Value {
        self.state.read().await.clone()
    }

    pub async fn get_state_string(&self) -> String {
        self.state.read().await.to_string()
    }

    pub async fn get_topic(&self, topic: &str) -> Option<Value> {
        self.state.read().await.get(topic).cloned()
    }

    pub async fn set_state(&self, new_state: Value) {
        *self.state.write().await = new_state;
    }

    pub async fn update_state(&self, update: Value) {
        merge(&mut *self.state.write().await, update);
    }
}

/// Merges a SignalR style partial update into `base`.
///
/// Objects are merged key by key. An object applied to an array is treated as
/// a sparse index map (`{"3": {...}}` updates the fourth element, or appends
/// when the index is past the end), which is how the F1 feed patches arrays
/// such as stints or race control messages.
pub fn merge(base: &mut Value, update: Value) {
    match (base, update) {
        (Value::Object(prev), Value::Object(update)) => {
            for (k, v) in update {
                merge(prev.entry(k).or_insert(Value::Null), v);
            }
        }
        (Value::Array(prev), Value::Object(update)) => {
            for (k, v) in update {
                if let Ok(index) = k.parse::<usize>() {
                    if let Some(item) = prev.get_mut(index) {
                        merge(item, v);
                    } else {
                        prev.push(v);
                    }
                }
            }
        }
        (a, b) => *a = b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_nested_objects() {
        let mut base = json!({"TimingData": {"Lines": {"1": {"Position": "1", "InPit": false}}}});
        merge(
            &mut base,
            json!({"TimingData": {"Lines": {"1": {"InPit": true}}}}),
        );
        assert_eq!(
            base,
            json!({"TimingData": {"Lines": {"1": {"Position": "1", "InPit": true}}}})
        );
    }

    #[test]
    fn patches_and_appends_array_items() {
        let mut base = json!({"Stints": [{"Compound": "SOFT", "TotalLaps": 3}]});
        merge(
            &mut base,
            json!({"Stints": {"0": {"TotalLaps": 4}, "1": {"Compound": "HARD"}}}),
        );
        assert_eq!(
            base,
            json!({"Stints": [{"Compound": "SOFT", "TotalLaps": 4}, {"Compound": "HARD"}]})
        );
    }

    #[test]
    fn replaces_scalars_and_arrays() {
        let mut base = json!({"a": [1, 2], "b": 1});
        merge(&mut base, json!({"a": [3], "b": "x"}));
        assert_eq!(base, json!({"a": [3], "b": "x"}));
    }
}
