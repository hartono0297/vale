# Settings → Widget: setting catatan tempel pindah ke halaman Catatan tempel

Date: 2026-11-27. Mockup: `docs/design/widget-settings-rapi.html` (disetujui user).

## Intent

Halaman Settings → Widget mencampur setting milik catatan tempel dengan setting
umum widget di satu kartu "Di desktop". User ingin setting milik catatan tempel
ada di setting catatan tempel sendiri, bukan di bagian umum widget, dan
halamannya rapi.

## Goals

- Halaman **Widget** hanya berisi: kartu dengan "Tampilkan widget" (saklar
  semua widget), lalu "Galeri" seperti sekarang.
- Klik kartu Catatan tempel di galeri membuka halaman **Catatan tempel** di
  dalam bagian Widget (seperti editor template di Fitur). Isinya:
  - "‹ Widget" untuk kembali.
  - Judul "Catatan tempel", "n di desktop", dan tombol (+) yang membuka menu
    template yang sama dengan (+) di galeri (mati di 20 catatan).
  - **Posisi**: Lapisan, Saat layar penuh, Kunci posisi.
  - **Tampilan**: Miring sedikit, Kucing ngintip.
  - **Catatan baru**: Isi dari clipboard.
- Kartu bisa diklik juga saat belum ada catatan. Saat ada catatan, klik tetap
  membuat catatan berkedip (perilaku lama dipertahankan), sekaligus membuka
  halaman.
- Pindah ke bagian lain menutup halaman Catatan tempel; kembali ke Widget
  membuka halaman galeri.
- Label dan deskripsi tiap setting tetap sama; data (`WidgetPrefs`, config
  key `widgets`) tidak berubah.

## Non-goals

- Tidak ada bagian (Section) baru di sidebar, tidak ada opsi `--settings` baru.
- Tidak mengubah menu klik kanan di catatan.
- Tidak mengubah arti atau default setting mana pun.

## Constraints

- Rust, Win32 + Direct2D, layout Settings di `src/ui/settings_layout.rs`,
  state dan klik di `src/ui/settings.rs`, teks di `src/i18n.rs` (Id + En).
- Halaman Catatan tempel muat tanpa scroll di 720x520 kalau bisa; halaman
  Widget pasti muat.
- Sesi paralel "Sticky note: maskot ikut pilihan user" mungkin mengubah baris
  "Kucing ngintip" di file yang sama; bentrok diselesaikan saat merge.

## Approaches

- **A, dipilih:** state "halaman Catatan tempel terbuka" di Settings, pola sama
  dengan `arrange_edit` di Fitur. Perubahan paling kecil.
- B: Section baru yang tersembunyi; lebih banyak yang berubah (daftar section,
  scroll, argumen).
- C: kartu melebar di tempat; halaman tetap scroll, tidak rapi.

## Left to the builder

Nama state/Hit, bentuk tombol kembali, jarak dan ukuran (ikuti
[[settings-spacing]]), tes layout dan klik.
