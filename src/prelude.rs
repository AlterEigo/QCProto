/// Универсальное возвращаемое значение с возможностью типизирования параметра
pub type UResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub use crate::network::*;
pub use crate::types::*;
