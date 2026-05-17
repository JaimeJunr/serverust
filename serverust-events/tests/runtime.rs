//! Testes para detecção de runtime (US-7).
//!
//! `Runtime::detect()` inspeciona `AWS_LAMBDA_FUNCTION_NAME` para decidir
//! entre execução em AWS Lambda ou processo long-running (ECS/EC2).
//!
//! Os testes manipulam variáveis de ambiente — agrupados em um único
//! `#[test]` para evitar race entre threads do harness paralelo.

use serverust_events::runtime::Runtime;

#[test]
fn detect_alterna_entre_lambda_e_long_running_via_env() {
    // SAFETY: manipulação de env é unsafe em Rust 2024. Os asserts
    // imediatos garantem que o estado é avaliado antes de qualquer
    // outro código tocar a env.
    unsafe {
        std::env::remove_var("AWS_LAMBDA_FUNCTION_NAME");
    }
    assert_eq!(Runtime::detect(), Runtime::LongRunning);
    assert!(Runtime::detect().is_long_running());
    assert!(!Runtime::detect().is_lambda());

    unsafe {
        std::env::set_var("AWS_LAMBDA_FUNCTION_NAME", "test-fn");
    }
    assert_eq!(Runtime::detect(), Runtime::Lambda);
    assert!(Runtime::detect().is_lambda());
    assert!(!Runtime::detect().is_long_running());

    unsafe {
        std::env::remove_var("AWS_LAMBDA_FUNCTION_NAME");
    }
    assert_eq!(Runtime::detect(), Runtime::LongRunning);
}

#[test]
fn runtime_implementa_traits_basicas() {
    fn assert_traits<T: Copy + Eq + std::fmt::Debug + Send + Sync>() {}
    assert_traits::<Runtime>();
}
