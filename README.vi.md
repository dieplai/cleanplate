<div align="center">

# cleanplate

### Xoá logo AI hiển thị, giữ nguyên hình. Không bịa chi tiết, không làm nhoè.

[![CI](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml/badge.svg)](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/dieplai/cleanplate?color=0f766e)](https://github.com/dieplai/cleanplate/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/dieplai/cleanplate/total?color=0f766e)](https://github.com/dieplai/cleanplate/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Platforms](https://img.shields.io/badge/Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-supported-555.svg)](#cài-đặt)
[![Dùng thử trên trình duyệt](https://img.shields.io/badge/d%C3%B9ng%20th%E1%BB%AD-tr%C3%AAn%20tr%C3%ACnh%20duy%E1%BB%87t-14b8a6.svg)](https://dieplai.github.io/cleanplate/)

[English](./README.md) · **Tiếng Việt**

</div>

<p align="center"><img src="docs/assets/hero_veo_legacy.gif" width="720" alt="Logo chữ Veo: bản gốc, GWT, wmr, cleanplate"></p>

cleanplate xoá các logo **hiển thị** mà công cụ AI đóng lên sản phẩm (chữ "Veo" của Google Veo và hình ✦ của
Gemini / Nano Banana) khỏi **video và ảnh**, trả lại hình trông như chưa từng có logo. Công cụ dành cho người làm nội
dung dùng các AI này cho mục đích phi thương mại, cần một khung hình sạch cho cảnh phim, bản dựng hay thumbnail.

Các tool khác đảo lớp phủ từng frame rồi che phần còn sót bằng làm mờ hoặc AI vẽ lại, nên để lại một mảng nhoè.
cleanplate tận dụng điều video vốn có: **cảnh chuyển động bên dưới logo, nên gần như mọi điểm ảnh bị che đều lộ ra
sạch sẽ ở một frame khác.** Nó dựng lại vùng logo từ chính các điểm ảnh thật đó. Với ảnh tĩnh, nó đo độ phủ và vị trí
chính xác của logo ngay trên ảnh của bạn thay vì giả định.

| Đo với đáp án, PSNR trong vùng logo (dB, càng cao càng tốt) | GWT 0.6.5 | wmr 1.17 | **cleanplate** | trần |
|---|---|---|---|---|
| Video, Veo 720p, 5 clip (trung bình) | 32.6 | — | **37.3** | 39.9 |
| Ảnh, Gemini ✦ PNG (gạch / neon) | 43.4 / 40.2 | 56.4 / 56.4 | **56.4 / 56.5** | 60 |
| Ảnh, lưu lại JPEG (gạch / neon) | 32.7 / 36.0 | 33.0 / 37.3 | **33.0 / 40.8** | 35.4 / 42.3 |
| Ảnh, thu nhỏ + JPEG như bản chia sẻ | không phát hiện | không phát hiện / sai | **30.4 / 34.1** | 32.3 / 43.9 |

<sub>Trần = cảnh sạch đi qua cùng codec; không công cụ nào vượt được. Từng clip và cách đo từng con số:
<a href="./docs/benchmark.md">docs/benchmark.md</a>.</sub>

> [!IMPORTANT]
> **Chỉ xoá logo hiển thị.** cleanplate không động tới SynthID (watermark ẩn) hay thông tin nguồn gốc C2PA. Hãy ghi
> nhãn nội dung do AI tạo theo quy định của nền tảng và kỳ vọng của người xem.

## Mục lục

[Kết quả](#kết-quả) · [Cài đặt](#cài-đặt) · [Cách dùng](#cách-dùng) · [Chất lượng tốt nhất](#để-có-chất-lượng-tốt-nhất) ·
[Phương pháp](#phương-pháp) · [Hiệu năng](#hiệu-năng) · [Giới hạn](#giới-hạn-đã-biết) · [Hỏi đáp](#hỏi-đáp) ·
[Ủng hộ](#-ủng-hộ) · [Giấy phép](#giấy-phép)

## Kết quả

![Tổng quan kết quả](./docs/assets/overview.png)

<table>
<tr>
<td width="50%"><img src="docs/assets/hero_veo.gif" alt="Logo Veo trên cảnh chợ hoa chuyển động"><br><sub>Veo 3.1, cảnh chuyển động: gốc · GWT · cleanplate</sub></td>
<td width="50%"><img src="docs/assets/hero_gemini.gif" alt="Logo Gemini trên phim hoạt hình chuyển động nhanh"><br><sub>Gemini ✦ trên chuyển động nhanh (20 px/frame): gốc · cleanplate</sub></td>
</tr>
</table>

<p align="center"><img src="docs/assets/hero_still.jpg" width="860" alt="Ảnh Flow: GWT sửa sai chỗ, wmr không phát hiện, cleanplate xoá được"><br>
<sub>Ảnh Flow (Nano Banana) thật với ✦ 48 px mờ. GWT vẽ một hình thoi tối sai chỗ; wmr không phát hiện ra logo.</sub></p>

## Cài đặt

### Cách 1 — dùng trên trình duyệt (không cần cài gì)

Mở **[dieplai.github.io/cleanplate](https://dieplai.github.io/cleanplate/)** bằng Chrome hoặc Edge rồi kéo file vào.
Mọi thứ chạy trên máy bạn, file không được tải lên đâu cả. Ảnh cho kết quả giống hệt CLI; video chậm hơn (một luồng)
và được nén lại bằng H.264.

### Cách 2 — tải công cụ dòng lệnh

| Hệ điều hành | Tải về | Sau đó |
|---|---|---|
| **Windows** 10/11 (x64) | [cleanplate-windows-x86_64.zip](https://github.com/dieplai/cleanplate/releases/latest/download/cleanplate-windows-x86_64.zip) | Giải nén; chạy `cleanplate.exe` từ terminal trong thư mục đó |
| **macOS** (Apple Silicon và Intel) | [cleanplate-macos-universal.tar.gz](https://github.com/dieplai/cleanplate/releases/latest/download/cleanplate-macos-universal.tar.gz) | `tar xzf`; chạy một lần: `xattr -d com.apple.quarantine cleanplate` |
| **Linux** x86_64 (glibc 2.35+) | [cleanplate-linux-x86_64.tar.gz](https://github.com/dieplai/cleanplate/releases/latest/download/cleanplate-linux-x86_64.tar.gz) | `tar xzf`; tuỳ chọn chép `cleanplate` vào `~/.local/bin` |

**Xử lý video cần thêm [ffmpeg](https://ffmpeg.org) trong PATH** (ảnh thì không cần):

| Hệ điều hành | Lệnh |
|---|---|
| Windows | `winget install --id Gyan.FFmpeg -e`, rồi mở một terminal **mới** |
| macOS | `brew install ffmpeg` |
| Ubuntu / Debian | `sudo apt install ffmpeg` |
| Fedora | `sudo dnf install ffmpeg-free` (không có x264: cleanplate sẽ xuất AV1) |

### Cách 3 — build từ mã nguồn

```bash
git clone https://github.com/dieplai/cleanplate.git && cd cleanplate
cargo install --path crates/cleanplate-cli      # Rust ≥ 1.88 (https://rustup.rs); ~1–3 phút
```

## Cách dùng

```bash
cleanplate photo.png photo_clean.png            # ảnh: vào PNG, JPG hoặc WebP; ra cùng hoặc khác định dạng
cleanplate clip.mp4 clip_clean.mp4              # video: H.264 (hoặc AV1), giữ âm thanh và thông tin màu
cleanplate clip.mp4 clip_clean.mov              # ProRes 422 HQ để dựng phim
cleanplate clip.mp4 clip_clean.mkv              # FFV1 lossless
cleanplate clip_1080p.mp4 out.mov --guide clip_720p_clean.mov   # bản tải AI upscale + bản 720p đã làm sạch
```

Trên Windows, khi chạy trong thư mục vừa giải nén thì gõ `.\cleanplate.exe` thay cho `cleanplate`.

| Tuỳ chọn | Ý nghĩa |
|---|---|
| `--codec auto\|h264x\|av1\|h264\|prores\|ffv1` | Bộ mã hoá video. `auto` chọn theo đuôi file ra và những gì ffmpeg của bạn có |
| `--guide FILE` | Với bản tải Flow 1080p / 4K: cùng clip ở 720p, đã làm sạch |
| `--threads N` | Số luồng (mặc định: tất cả nhân CPU) |
| `--lam`, `--k`, `--min-ncc`, `--opaque`, `--no-edge-tv` | Tinh chỉnh nâng cao; xem `cleanplate --help` |

**Mã thoát:** `0` đã xoá · `2` không tìm thấy logo đủ chắc chắn, **không ghi gì** (file gốc không bao giờ bị sửa) ·
`1` lỗi (thông báo nói rõ cần làm gì, ví dụ cách cài ffmpeg).

## Để có chất lượng tốt nhất

- **Tải bản độ phân giải gốc.** Trong Flow chọn 720p "original size". Bản 1080p và 4K của Flow là AI upscale từ video
  *đã có logo*: logo bị vẽ lại và phá một phần chi tiết bên dưới. Nếu chỉ có bản 1080p, hãy làm sạch thêm bản 720p
  rồi truyền vào bằng `--guide`.
- **Dùng file đúng như lúc tải về.** Mỗi lần nén lại (app nhắn tin, mạng xã hội, quay màn hình) làm mất chi tiết mà
  phương pháp cần. cleanplate vẫn xử lý được bản đã thu nhỏ hoặc nén lại, nhưng độ chính xác thấp hơn.
- **Lưu ảnh dạng PNG.** PNG không mất dữ liệu; xuất JPEG sẽ nén lại toàn bộ ảnh.
- **Để dựng phim, xuất `.mov` (ProRes) hoặc `.mkv` (FFV1)** để công cụ tiếp theo nhận file không mất dữ liệu.

## Phương pháp

```
video ─► dò      khớp nhiều mẫu NCC tại vị trí quen thuộc của từng công cụ; không chắc thì từ chối
      ─► cảnh    phát hiện điểm cắt cảnh; không bao giờ mượn điểm ảnh qua điểm cắt
      ─► mỗi frame, mỗi điểm ảnh trong logo:
            đảo lớp phủ trong không gian YUV gốc         I = (J − a·L) / (1 − a)
            lấy cùng điểm cảnh từ ±K frame lân cận       optical flow DIS + kiểm tra xuôi/ngược
            trọng số: điểm sạch ≫ điểm đảo (1 − a)², × độ tin cậy flow × suy giảm theo thời gian
            loại ngoại lai thô, trung bình bền vững
            MAP với TV có trọng số chỉ ở nơi chưa frame nào thấy nền
      ─► dán     làm mềm viền, vào frame không bị đụng tới; mã hoá; giữ âm thanh và thông tin màu

ảnh  ─► dò ✦ ở mọi kích thước ─► đo độ phủ, độ lệch dưới 1 pixel và kích thước trên chính ảnh ─► đảo chính xác
     ─► TV thích ứng theo kết cấu cho ảnh JPEG / WebP
```

| Trường hợp | Cách xử lý |
|---|---|
| Cảnh chuyển động | Gom điểm ảnh thật từ frame lân cận theo optical flow, gộp theo nghịch đảo phương sai |
| Cảnh tĩnh | Trung bình phép đảo trên cả cảnh + MAP TV có trọng số; λ chọn theo chuyển động đo được của từng cảnh |
| Chuyển động nhanh | Loại ngoại lai so với phép đảo của chính frame; TV ở viền logo nơi không có điểm sạch |
| Màu | YUV gốc theo vị trí lấy mẫu màu của H.264; không đổi qua RGB |
| Ảnh | Đo độ phủ ngay trên ảnh (hai kiểu ✦, độ phủ 0.30 và 0.50); ảnh thu nhỏ qua kích thước lẻ |
| Logo | Chữ Veo (720p, 1080p, bản cũ 68 px), Gemini ✦ 36 / 48 / 96 px, ảnh Flow (Nano Banana) |
| An toàn | Không khớp đủ chắc thì không sửa file (14 / 14 file sạch bị từ chối); input quá nhỏ cũng bị từ chối |

Bằng chứng cho từng bước và các ý tưởng đã thử rồi loại: [`docs/method.md`](./docs/method.md).

Tham khảo: Dekel và cộng sự, *On the Effectiveness of Visible Watermarks*, CVPR 2017 · E2FGVI (CVPR 2022) và
ProPainter (ICCV 2023) cho lan truyền theo flow · Kroeger và cộng sự, optical flow DIS, ECCV 2016 · TV
Rudin–Osher–Fatemi, 1992.

## Hiệu năng

Trên laptop 12 nhân: clip 720p dài 8 giây mất **3–9 giây** (cảnh tĩnh 9:16 mất 18 giây), ảnh 2528×1696 mất **2 giây**:
**nhanh hơn bản Python tham chiếu khoảng 16 lần với cùng chất lượng** (cổng đo có đáp án trên 8 clip: trung bình 36,65
so với 36,64 dB; ảnh giống hệt từng bit; [`bench/rust_vs_python.sh`](./bench/rust_vs_python.sh)). Bản trình duyệt chạy
cùng core dưới dạng WebAssembly trên một luồng: ~9 giây cho ảnh đó, ~21 giây cho clip đó.

Mã Rust chia thành [`cleanplate-core`](./crates/cleanplate-core) (toàn bộ thuật toán, không đụng file hay hệ điều
hành) và [`cleanplate-cli`](./crates/cleanplate-cli) (đọc ghi file và ffmpeg);
[`cleanplate-wasm`](./crates/cleanplate-wasm) + [`web/`](./web) là trang trình duyệt. Mã Python trong
[`research/`](./research) là đặc tả tham chiếu và bộ benchmark.

## Giới hạn đã biết

- **Cảnh tĩnh bị giới hạn thông tin.** Nếu nền dưới logo không bao giờ lộ ra, cleanplate trả về hình có khả năng
  đúng nhất, không phải chi tiết mà codec đã vứt bỏ (trường hợp xấu nhất: cảnh bếp 9:16, 36,2 so với 41,9 dB).
- **Bản tải AI upscale** (Flow 1080p / 4K) vẽ lại logo theo từng nền; không có `--guide` 720p thì cảnh tĩnh còn một
  mảng mờ nhẹ.
- **Logo trên nền trắng tuyệt đối** (≤ 7 mức trên 253) không công cụ nào phát hiện được, kể cả cleanplate; khi đó
  logo cũng gần như vô hình.
- Video trên trình duyệt cần Chrome hoặc Edge và file MP4 / MOV H.264; định dạng khác hãy dùng CLI.

## Hỏi đáp

**Có xoá SynthID hay C2PA không?** Không. Chỉ xoá logo hiển thị. SynthID giữ nguyên, còn chữ ký C2PA không thể còn hợp
lệ sau bất kỳ lần nén lại nào.

**File của tôi không bị sửa (mã thoát 2).** cleanplate không tìm thấy logo đủ chắc chắn nên không đoán bừa. Điều này
xảy ra với file sạch, bản rất nhỏ hoặc bị nén lại nhiều lần, và logo trên nền trắng. Nếu bạn vẫn thấy logo, hãy
[mở issue](https://github.com/dieplai/cleanplate/issues) kèm file.

**Windows báo "Windows protected your PC".** File chạy chưa được ký số. Chọn *More info → Run anyway*, hoặc build từ
mã nguồn.

**File có bị tải lên đâu không?** Không. CLI chạy offline; trang web xử lý file ngay trong tab trình duyệt.

## 💖 Ủng hộ

Nếu cleanplate giúp bạn tiết kiệm thời gian, hãy tặng dự án một ⭐ trên GitHub hoặc tài trợ để phát triển tiếp:

<a href="https://github.com/sponsors/dieplai"><img src="https://img.shields.io/badge/Sponsor-dieplai-EA4AAA?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Tài trợ dieplai trên GitHub" height="40"></a>

## Giấy phép

| Phần | Giấy phép |
|---|---|
| Mã nguồn | [MIT](./LICENSE) |
| Mẫu logo Veo 720p / 1080p (cleanplate tự trích xuất) | MIT |
| Mẫu ✦ Gemini và Veo bản cũ | MIT, © Frédéric Guigand, từ [froggeric/gemini-watermark-and-synthid-remover](https://github.com/froggeric/gemini-watermark-and-synthid-remover); xem [ATTRIBUTION](./assets/marks/google/ATTRIBUTION.md) |
| Bố cục trang web, mp4box, mp4-muxer | Xem [web/THIRD_PARTY.md](./web/THIRD_PARTY.md) |

Mọi đóng góp đều được hoan nghênh: đọc [CONTRIBUTING.md](./CONTRIBUTING.md). Muốn khẳng định "tốt hơn" cần có số đo từ
`research/score_gt.py` hoặc `research/img_bench.py` và hình so sánh trực quan.
