# Clipboard Diary

Clipboard manager native cho Windows, viet bang Rust + `egui/eframe`.

## Chay app

Tu thu muc goc project:

```powershell
cargo run
```

App se mo cua so desktop native va theo doi clipboard tren Windows.

## Test nhanh

1. Chay `cargo run`.
2. Mo Notepad, VS Code hoac browser roi copy vai doan text khac nhau.
3. Quay lai app de xem lich su clipboard duoc them vao danh sach.
4. Double click mot dong de copy lai.
5. Chuot phai vao item de thu `Copy to clipboard` hoac `Delete`.

## Build kiem tra

```powershell
cargo check
```

## Cau truc

- `src/lib.rs`: logic app egui + clipboard history
- `src/main.rs`: entry point native
- `Cargo.toml`: dependencies Rust
