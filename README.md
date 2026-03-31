# Clipboard Diary

Clipboard manager native cho Windows, viết bằng Rust + `egui/eframe`, đi theo hướng giao diện giống Clipdiary.

## Chạy app để test

Từ thư mục gốc project:

```powershell
cargo run --manifest-path src-tauri/Cargo.toml
```

App sẽ mở cửa sổ desktop native và tự theo dõi clipboard text của Windows.

## Cách test nhanh

1. Chạy lệnh ở trên.
2. Mở Notepad, VS Code hoặc browser rồi copy vài đoạn text khác nhau.
3. Quay lại app để xem lịch sử clipboard được thêm vào danh sách.
4. Double click một dòng để copy lại.
5. Chuột phải vào item để thử `Copy to clipboard` hoặc `Delete`.

## Build kiểm tra

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

## Cấu trúc còn lại

- `src-tauri/src/lib.rs`: logic app egui + clipboard history
- `src-tauri/src/main.rs`: entry point native
- `src-tauri/Cargo.toml`: dependencies Rust
