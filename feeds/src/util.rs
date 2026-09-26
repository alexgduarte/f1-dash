use tokio::task::JoinHandle;

/// Aborts a task when dropped, tying a background task's life to its owner.
pub struct AbortOnDrop<T>(pub JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
