use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serverust_macros::injectable;

use super::model::{CreateFundDto, Fund};

#[injectable]
pub struct FundsService {
    funds: Mutex<Vec<Fund>>,
    next_id: AtomicU64,
}

impl FundsService {
    pub fn new() -> Self {
        Self {
            funds: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn list(&self) -> Vec<Fund> {
        self.funds.lock().unwrap().clone()
    }

    pub fn get(&self, id: u64) -> Option<Fund> {
        self.funds
            .lock()
            .unwrap()
            .iter()
            .find(|f| f.id == id)
            .cloned()
    }

    pub fn create(&self, dto: CreateFundDto) -> Fund {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let fund = Fund {
            id,
            name: dto.name,
            cnpj: dto.cnpj,
            nav: dto.nav,
            created_at: chrono_now(),
        };
        self.funds.lock().unwrap().push(fund.clone());
        fund
    }

    pub fn update(&self, id: u64, dto: CreateFundDto) -> Option<Fund> {
        let mut funds = self.funds.lock().unwrap();
        let fund = funds.iter_mut().find(|f| f.id == id)?;
        fund.name = dto.name;
        fund.cnpj = dto.cnpj;
        fund.nav = dto.nav;
        Some(fund.clone())
    }

    pub fn delete(&self, id: u64) -> bool {
        let mut funds = self.funds.lock().unwrap();
        let len_before = funds.len();
        funds.retain(|f| f.id != id);
        funds.len() < len_before
    }
}

impl Default for FundsService {
    fn default() -> Self {
        Self::new()
    }
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{secs}")
}
