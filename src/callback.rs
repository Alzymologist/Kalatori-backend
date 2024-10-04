use crate::server::definitions::api_v2::OrderStatus;
use tokio::task;

pub const MODULE: &str = module_path!();

// TODO: This will be used once we setup callback functionality
#[allow(dead_code)]
pub async fn callback(path: String, order_status: OrderStatus) {
    let req = ureq::post(&path);

    task::spawn_blocking(move || {
        let _d = req.send_json(order_status);
    })
    .await
    .unwrap();
}
