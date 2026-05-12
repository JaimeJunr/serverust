use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Fund {
    pub id: u64,
    pub name: String,
    pub cnpj: String,
    pub nav: f64,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateFundDto {
    #[validate(length(min = 1, max = 200))]
    #[schema(min_length = 1, max_length = 200)]
    pub name: String,
    /// CNPJ no formato XX.XXX.XXX/XXXX-XX
    #[validate(length(min = 14, max = 18))]
    #[schema(min_length = 14, max_length = 18)]
    pub cnpj: String,
    /// Valor líquido de ativos (non-negative)
    #[validate(range(min = 0.0))]
    pub nav: f64,
}
