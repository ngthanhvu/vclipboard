# Clipboard Diary

Clipboard manager native cho Windows, viet bang Rust + `egui/eframe`.

## Yeu cau

Project nay huong toi Windows.

Can cai san:

- Rust toolchain `stable-x86_64-pc-windows-msvc`
- Microsoft C++ Build Tools / Visual Studio Build Tools
- `cargo-packager`
- `NSIS` neu muon build installer `.exe`

Cai `cargo-packager`:

```powershell
cargo install cargo-packager
```

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

## Build release

Build file app release:

```powershell
cargo build --release
```

File tao ra:

- `target\release\Vclipboard.exe`

Luu y:

- App hien tai dung renderer `wgpu` de tranh loi OpenGL tren mot so may Windows cu / driver yeu.
- Neu copy file `.exe` thuan sang may khac thi van nen test thuc te tren may dich.

## Dong goi installer Windows

Project dang cau hinh `cargo-packager` voi format `nsis` trong `Cargo.toml`, nen output installer hien tai la file cai dat `.exe`.

Build installer:

```powershell
cargo packager --release
```

Truoc khi chay lenh nay, can dam bao `NSIS` da duoc cai va `makensis.exe` co trong `PATH`.

Khuyen nghi phat hanh:

- Dung file installer do `cargo packager --release` tao ra de gui cho nguoi dung
- Khong nen chi gui moi `target\release\Vclipboard.exe` lam ban cai dat chinh

## Debug tren may nguoi dung

App co ghi log runtime de debug ban release.

Khi gap loi tren may khac:

1. Mo app.
2. Tim file `runtime.log` trong thu muc data cua app.
3. Gui lai noi dung log de kiem tra loi khoi dong / renderer / tray / window visibility.

Neu log bao loi lien quan toi renderer OpenGL, ban hien tai da duoc chuyen sang `wgpu` de tuong thich tot hon tren Windows.

## Cau truc

- `src/lib.rs`: logic app egui + clipboard history
- `src/main.rs`: entry point native
- `Cargo.toml`: dependencies Rust
