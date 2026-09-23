use std::collections::HashMap;

/// 按群冷却。check 不写时间，update 在动作成功后调用。
#[derive(Default)]
pub struct Cooldown {
    timestamps: HashMap<i64, f64>,
}

impl Cooldown {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check(&self, group_id: i64, now: f64, cooldown_secs: f64) -> bool {
        let last = self.timestamps.get(&group_id).copied().unwrap_or(0.0);
        now - last >= cooldown_secs
    }

    pub fn update(&mut self, group_id: i64, now: f64) {
        self.timestamps.insert(group_id, now);
    }
}
