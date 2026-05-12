use funds_api::modules;

use std::sync::Arc;

use modules::funds::{
    handlers::{create_fund, delete_fund, get_fund, list_funds, update_fund},
    model::{CreateFundDto, Fund},
    service::FundsService,
};
use serverust_lambda::AppRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serverust_core::App::new()
        .openapi_info("Funds API", "1.0.0")
        .register_schema::<Fund>()
        .register_schema::<CreateFundDto>()
        .provide::<FundsService>(Arc::new(FundsService::new()))
        .route(list_funds)
        .route(create_fund)
        .route(get_fund)
        .route(update_fund)
        .route(delete_fund)
        .run()
        .await?;
    Ok(())
}
