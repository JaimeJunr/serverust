use serverust_lambda::AppRuntime;
use serverust_macros::get;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serverust_core::App::new().route(hello).run().await?;
    Ok(())
}
