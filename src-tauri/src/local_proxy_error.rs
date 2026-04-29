use thiserror::Error;

#[derive(Debug, Error)]
pub enum LocalProxyTransformError {
    #[error("格式转换错误: {0}")]
    TransformError(String),
}
