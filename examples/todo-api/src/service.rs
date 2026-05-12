use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serverust_macros::injectable;

use crate::model::{CreateTaskDto, Task, UpdateTaskDto};

/// Service em memória das tarefas. Em produção, troque por um repositório que
/// fale com banco/Dynamo; a assinatura pública é a mesma.
#[injectable]
pub struct TaskService {
    tasks: Mutex<Vec<Task>>,
    next_id: AtomicU64,
}

impl TaskService {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn list(&self) -> Vec<Task> {
        self.tasks.lock().unwrap().clone()
    }

    pub fn get(&self, id: u64) -> Option<Task> {
        self.tasks
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned()
    }

    pub fn create(&self, dto: CreateTaskDto) -> Task {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let task = Task {
            id,
            title: dto.title,
            done: false,
            created_at: now_epoch_secs(),
        };
        self.tasks.lock().unwrap().push(task.clone());
        task
    }

    pub fn update(&self, id: u64, dto: UpdateTaskDto) -> Option<Task> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks.iter_mut().find(|t| t.id == id)?;
        if let Some(title) = dto.title {
            task.title = title;
        }
        if let Some(done) = dto.done {
            task.done = done;
        }
        Some(task.clone())
    }

    pub fn delete(&self, id: u64) -> bool {
        let mut tasks = self.tasks.lock().unwrap();
        let before = tasks.len();
        tasks.retain(|t| t.id != id);
        tasks.len() < before
    }
}

impl Default for TaskService {
    fn default() -> Self {
        Self::new()
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
