use goble_core::store::Store;
use tempfile::TempDir;

fn main() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("worker.db");
    let store = Store::open(&path).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    store
        .insert_chat("chat-1", "test", None, None, &now, &now)
        .unwrap();
    store
        .insert_chat_message(
            &uuid::Uuid::new_v4().to_string(),
            "chat-1",
            "user",
            "hello",
            None,
            &now,
        )
        .unwrap();
    println!("ok {}", path.display());
}
