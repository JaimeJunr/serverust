use serverust_lambda::AppRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Em ambiente local: serve em 0.0.0.0:3000.
    // Em AWS Lambda: consome eventos do API Gateway / Function URL automaticamente.
    todo_api::build_app().run().await?;
    Ok(())
}
