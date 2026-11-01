# Mendaftarkan Vale ke Google (login dengan Gmail)

Panduan untuk pemilik repo: supaya pengguna bisa klik **Masuk dengan Google**
dan login dengan akun Gmail mereka sendiri, tanpa membuat Client ID sendiri.

Ringkasnya ada tiga tahap:

1. **Siapkan proyek Google** dan buat client "Desktop app" (± 30 menit).
2. **Masukkan client itu ke build rilis** lewat GitHub Secrets, lalu rilis.
   Setelah ini tombol "Masuk dengan Google" muncul dan sudah bisa dipakai,
   tapi Google menampilkan peringatan "Google hasn't verified this app" dan
   jumlah pengguna dibatasi (jalur **B**).
3. **Verifikasi aplikasi** di Google supaya peringatan hilang dan tidak ada
   batas pengguna (jalur **C**): butuh website dengan domain sendiri,
   kebijakan privasi, video demo, lalu menunggu tim Google.

Vale meminta dua izin Calendar, keduanya termasuk *sensitive scope*:

| Scope | Untuk apa |
|---|---|
| `https://www.googleapis.com/auth/calendar.calendarlist.readonly` | Membaca daftar kalender (mana yang dicentang) |
| `https://www.googleapis.com/auth/calendar.events` | Membaca, menambah dan mengubah acara |

---

## Tahap 1 · Proyek Google dan client

1. **Pakai akun Google yang akan "memiliki" aplikasi.** Sebaiknya akun yang
   dipakai jangka panjang (bukan akun coba-coba); email ini terlihat oleh
   pengguna sebagai kontak dukungan.
2. Buka [console.cloud.google.com](https://console.cloud.google.com/) →
   pilih proyek di kiri atas → **New Project** → nama **Vale** → *Create*.
3. **Aktifkan API:** menu **APIs & Services → Library** → cari
   **Google Calendar API** → **Enable**.
4. **Google Auth Platform** (dulu *OAuth consent screen*) → **Get started**:
   - *App name*: **Vale**
   - *User support email*: email dukungan Anda
   - *Audience*: **External**
   - *Contact information*: email Anda → setujui kebijakan → *Create*.
5. **Branding** (menu kiri):
   - *App logo*: boleh dikosongkan dulu. Mengunggah logo membuat Google
     meminta verifikasi merek, jadi unggah saat masuk tahap 3.
   - *Application home page*: alamat website Vale (tahap 3), misalnya
     `https://vale.domainanda.com`. Untuk tahap 2 boleh halaman GitHub repo.
   - *Privacy policy* dan *Terms of service*: alamat `privacy.html` dan
     `terms.html` dari folder `site/` (lihat tahap 3).
   - *Authorized domains*: domain website Anda (tanpa `https://`).
6. **Data access** → **Add or remove scopes** → centang (atau tempel di
   *Manually add scopes*) dua scope di tabel atas → *Update* → *Save*.
7. **Clients** → **Create client**:
   - *Application type*: **Desktop app**
   - *Name*: `Vale desktop` → *Create*.
   - **Salin Client ID dan Client secret** sekarang (secret mungkin hanya
     ditampilkan sekali; kalau hilang, buat secret baru di halaman client).
   - Untuk aplikasi desktop, Google tidak menganggap secret ini rahasia
     (ia ikut tertanam di exe), jadi aman dimasukkan ke build rilis.
8. **Audience** → *Publishing status* masih **Testing**. Di **Test users** →
   *Add users*: tambahkan akun Gmail Anda dan siapa pun yang mau mencoba.
   Selama *Testing*, hanya test user yang bisa login, dan login mereka
   kedaluwarsa setelah 7 hari (Vale lalu meminta "Hubungkan" lagi).

## Tahap 2 · Masukkan ke rilis Vale (jalur B)

1. Di GitHub: repo **hartono0297/vale** → **Settings** →
   **Secrets and variables** → **Actions** → **New repository secret**:
   - `VALE_GOOGLE_CLIENT_ID` = Client ID dari langkah 1.7
   - `VALE_GOOGLE_CLIENT_SECRET` = Client secret dari langkah 1.7
2. Buat rilis baru (push tag `vX.Y.Z`). Workflow `release.yml` membaca
   kedua secret itu saat build, dan exe rilis kini punya client bawaan:
   di **Pengaturan → Integrasi** muncul tombol **Masuk dengan Google**.
   Build lokal (tanpa secret) tetap hanya menampilkan opsi Client ID sendiri.
3. Coba sendiri dengan akun test user: klik **Masuk dengan Google** → pilih
   akun → centang **kedua** izin → kalender muncul di panel.
4. **Supaya semua orang bisa login (bukan hanya test user):**
   **Audience → Publish app** → status menjadi **In production**.
   - Login kini tahan lama (tidak lagi 7 hari).
   - Tapi karena scope-nya *sensitive* dan aplikasi belum diverifikasi,
     pengguna melihat layar **"Google hasn't verified this app"**; mereka
     harus klik **Advanced → Go to Vale (unsafe)**.
   - Google juga membatasi jumlah akun yang boleh login ke aplikasi yang
     belum diverifikasi (user cap; di dokumentasi Google angkanya 100).
   - Jalur ini cukup untuk Anda dan teman-teman; untuk publik, lanjut tahap 3.

## Tahap 3 · Verifikasi oleh Google (jalur C)

Yang diminta Google untuk *sensitive scope*:

1. **Website publik dengan domain sendiri.**
   - Isi placeholder di `site/` (`[support email]`, `[license]`, tanggal),
     lalu host folder `site/` (mis. GitHub Pages dengan custom domain, atau
     hosting lain). Halaman beranda harus bisa dibuka tanpa login dan jelas
     menjelaskan Vale.
   - `privacy.html` harus di **domain yang sama** dengan beranda, dan
     menjelaskan data Google apa yang diakses, untuk apa, di mana disimpan,
     dan cara menghapusnya. Draf di `site/privacy.html` sudah memuat ini,
     termasuk kalimat *Google API Services User Data Policy / Limited Use*.
2. **Verifikasi kepemilikan domain** di
   [Google Search Console](https://search.google.com/search-console) dengan
   akun yang sama (Owner/Editor proyek). Tambahkan domain itu di
   *Branding → Authorized domains*.
3. **Branding lengkap:** nama **Vale**, logo (120 × 120 px, file
   `assets/logo-256.png` diperkecil), email dukungan, link beranda, privacy
   dan terms, semuanya sesuai website.
4. **Video demo** (bahasa Inggris), unggah ke YouTube sebagai **Unlisted**:
   - mulai dari Vale → **Pengaturan → Integrasi → Masuk dengan Google**;
   - tampilkan halaman login Google dan **consent screen dengan nama "Vale"**;
   - tunjukkan **address bar browser yang memuat `client_id=`** (zoom kalau
     perlu);
   - tunjukkan fungsi tiap scope: daftar kalender terbaca
     (`calendarlist.readonly`), lalu tambah dan ubah satu acara dari editor
     Vale dan tampilkan hasilnya di Google Calendar (`calendar.events`).
5. **Alasan tiap scope** (bisa dipakai apa adanya):
   - `calendar.calendarlist.readonly`: *"Vale shows the user's Google
     Calendar events in a panel at the top of the screen. It reads the
     calendar list only to know which calendars the user has selected, so
     their events can be shown. It never changes the list. No narrower scope
     returns the selected calendars."*
   - `calendar.events`: *"Users view their events by day, week or month and
     can add or edit an event from Vale's event editor. Writing events needs
     calendar.events; calendar.events.readonly would not allow adding or
     editing. Vale never deletes events and has no server: tokens and data
     stay on the user's PC."*
6. **Kirim:** **Google Auth Platform → Verification Center** → *Prepare for
   verification* / *Submit*, isi semua kolom di atas.
7. **Tunggu dan pantau email.** Google menyebut 3–10 hari kerja untuk
   *sensitive scope*; kenyataannya bisa lebih lama. Tim *Trust & Safety*
   bisa meminta perbaikan lewat email (balas dari akun yang sama).
8. Setelah disetujui: peringatan "unverified" hilang dan batas pengguna
   dicabut. Tidak perlu rilis ulang Vale; client ID-nya sama.

## Catatan

- Token login pengguna tersimpan di PC mereka, terenkripsi DPAPI
  (`%APPDATA%\Vale\google-token.bin`). Vale tidak punya server.
- Mengganti client (mis. membuat client baru) membuat pengguna diminta login
  lagi dengan pesan yang jelas; client lama sebaiknya tidak dihapus sampai
  rilis baru tersebar.
- Pengguna yang lebih suka memakai project Google mereka sendiri tetap bisa
  memakai **Lanjutan: pakai Client ID sendiri**.

Sumber: [Sensitive scope verification (Google)](https://developers.google.com/identity/protocols/oauth2/production-readiness/sensitive-scope-verification),
[OAuth verification help (Google)](https://support.google.com/cloud/answer/13463817).
