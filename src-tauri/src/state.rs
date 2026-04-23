use crate::models::AppStatus;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct StateStore {
    pub inner: RwLock<AppStatus>,
}
