<div align="center">

# cleanplate

### Xoá logo nhìn thấy được, giữ nguyên hình ảnh. Không bịa thêm, không làm mờ.

[![CI](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml/badge.svg)](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-core%20%2B%20CLI-B7410E.svg)](./crates)
[![Status: research preview](https://img.shields.io/badge/status-research%20preview-orange.svg)](./CHANGELOG.md)

[English](./README.md) · **Tiếng Việt**

</div>

<p align="center"><img src="docs/assets/hero_veo_legacy.gif" width="720" alt="Logo chữ Veo: gốc, GWT, wmr, cleanplate"></p>

**1. Đây là gì.** cleanplate xoá các logo *nhìn thấy được* mà công cụ AI đóng lên sản phẩm (chữ "Veo" của Google Veo,
hình thoi ✦ của Gemini) trên **cả video lẫn ảnh**, và trả lại hình như chưa từng có logo. Dự án dành cho người sáng
tạo làm sản phẩm phi thương mại với các công cụ này, cần một cảnh sạch để dựng phim, chỉnh sửa hay làm thumbnail.

**2. Vì sao cần thêm một tool nữa.** Các tool hiện có đảo lớp phủ từng frame rồi che phần còn sót bằng làm mờ hoặc AI
vẽ lại, nên vùng logo hay bị mờ mờ. Video có một thứ mà ảnh không có: **cảnh nền di chuyển bên dưới logo, nên gần
như mọi điểm bị che đều lộ ra, sạch, ở một frame khác.** cleanplate dựng lại vùng logo từ chính những pixel thật đó,
và chỉ dùng ước lượng MAP ở những chỗ không frame nào nhìn thấy nền. Với ảnh, tool tự đo độ phủ của logo ngay trên
ảnh thay vì tin vào một template cố định.

| Có đáp án, PSNR trong vùng logo (dB) | GWT 0.6.5 | wmr 1.17 | **cleanplate** | mức trần |
|---|---|---|---|---|
| Video Veo 720p, 5 clip (trung bình) | 32.6 | — | **37.3** | 39.9 |
| Ảnh Gemini ✦ PNG (gạch / neon) | 43.4 / 40.2 | 56.4 / 56.4 | **56.4 / 56.5** | 60 |
| Ảnh lưu lại JPEG (gạch / neon) | 32.7 / 36.0 | 33.0 / 37.3 | **33.0 / 40.8** | 35.4 / 42.3 |
| Ảnh thu nhỏ + JPEG, kiểu ảnh gửi qua mạng | không nhận ra | không nhận ra / nhận sai | **30.4 / 34.1** | 32.3 / 43.9 |

<sub>Mức trần = chính cảnh sạch đi qua cùng bộ nén: không tool nào vượt được. Bảng đầy đủ, từng clip và cách đo:
<a href="./docs/benchmark.md">docs/benchmark.md</a>.</sub>

> **Chỉ xoá logo nhìn thấy được.** cleanplate không cố xoá SynthID (watermark ẩn) hay thông tin nguồn gốc C2PA.
> Việc nén lại không thể giữ chữ ký C2PA còn hiệu lực (chữ ký gắn với pixel gốc); metadata của container video được
> chép sang, metadata của ảnh thì không. Hãy ghi chú nội dung do AI tạo theo quy định của nền tảng và người xem.

## Kết quả tổng quan

![Kết quả tổng quan](./docs/assets/overview.png)

<table>
<tr>
<td width="50%"><img src="docs/assets/hero_veo.gif" alt="Logo Veo trên cảnh chợ hoa chuyển động"><br><sub>Veo 3.1, cảnh chuyển động: gốc · GWT · cleanplate</sub></td>
<td width="50%"><img src="docs/assets/hero_gemini.gif" alt="Logo Gemini trên phim hoạt hình chuyển động nhanh"><br><sub>Gemini ✦, chuyển động nhanh (20 px/frame): gốc · cleanplate</sub></td>
</tr>
</table>

<p align="center"><img src="docs/assets/hero_still.jpg" width="860" alt="Ảnh Flow: GWT sửa sai chỗ, wmr không nhận ra, cleanplate xoá sạch"><br>
<sub>Ảnh thật từ Flow (Nano Banana), logo ✦ 48 px rất mờ. GWT tô một hình thoi tối vào sai chỗ và để nguyên logo; wmr không nhận ra.</sub></p>

## Xử lý được những gì

| Trường hợp | Cách làm |
|---|---|
| Cảnh chuyển động | Lấy pixel thật từ các frame lân cận theo optical flow (DIS, kiểm tra hai chiều, gộp theo nghịch đảo phương sai) |
| Cảnh tĩnh | Trung bình phép đảo trên cả cảnh + MAP–TV có trọng số; λ chọn theo chuyển động đo được của từng cảnh |
| Chuyển động nhanh | Loại ứng viên sai lệch lớn so với phép đảo của chính frame đó; TV trên viền logo ở chỗ không có pixel sạch |
| Chuyển cảnh | Tự phát hiện cắt cảnh; không bao giờ lấy pixel qua chỗ cắt |
| Màu sắc | Xử lý trực tiếp trên YUV gốc theo chuẩn đặt chroma của H.264; không chuyển qua RGB |
| Ảnh | Tự đo độ phủ, độ lệch dưới 1 pixel và kích thước lẻ (với ảnh đã thu nhỏ); làm mịn theo độ chi tiết cho JPEG |
| Các loại logo | Chữ Veo (720p, bản cũ 68 px), Gemini ✦ 36/48/96 px ở cả hai phiên bản (độ phủ 0.30 và 0.50), ảnh Flow |
| An toàn | Phải nhận ra logo chắc chắn, không thì để nguyên file (14/14 file sạch bị từ chối) |
| Xuất file | Giữ âm thanh và thông tin màu; ProRes 422 HQ, AV1 10-bit, H.264 hoặc FFV1 không mất dữ liệu; ảnh giữ định dạng gốc |

## Bắt đầu nhanh

```bash
git clone https://github.com/dieplai/cleanplate.git && cd cleanplate
cargo install --path crates/cleanplate-cli        # Rust ≥ 1.88; video cần thêm ffmpeg trong PATH

cleanplate anh.png anh_sach.png
cleanplate clip.mp4 clip_sach.mov                 # .mov = ProRes 422 HQ; hoặc --codec av1 / h264 / ffv1
cleanplate clip_1080p.mp4 ra.mov --guide clip_720p_sach.mov      # bản AI upscale + bản 720p đã làm sạch
```

Một clip 720p dài 8 giây mất **3–9 giây** (cảnh tĩnh 9:16 mất 18 giây), một ảnh 2528×1696 mất **2 giây**, trên laptop
12 nhân. **Nhanh hơn bản Python khoảng 16 lần với cùng chất lượng** (cổng kiểm tra có đáp án: trung bình 36.65 so với
36.64 dB trên 8 clip; ảnh trùng khớp; `bench/rust_vs_python.sh`). File không có logo không bao giờ bị sửa: tool thoát với
mã 2 và không ghi gì.

**Để có chất lượng cao nhất:** tải bản **độ phân giải gốc** (trên Flow là 720p "Kích thước gốc"). Bản 1080p và 4K của
Flow là bản AI upscale từ video **đã có logo**, nên logo bị vẽ lại và làm mất một phần nội dung bên dưới. Nếu bạn chỉ
có bản 1080p, hãy đưa thêm bản 720p của cùng clip qua `--guide`.

`crates/cleanplate-core` chứa toàn bộ thuật toán, không đọc/ghi file hay gọi hệ điều hành (sẵn sàng cho WebAssembly);
`crates/cleanplate-cli` lo phần đọc/ghi. Code Python trong `research/` được giữ làm đặc tả tham chiếu và benchmark
nghiên cứu (`pip install numpy opencv-python`).

### Trên trình duyệt

Thư mục `web/` là một trang kéo-thả tĩnh chạy chính core Rust dưới dạng WebAssembly (ảnh và video MP4/MOV; không
upload gì cả). Mở bằng Chrome hoặc Edge (video cần WebCodecs):

```bash
python3 -m http.server 8765 --directory web       # rồi mở http://localhost:8765
./web/build.sh                                    # sau khi sửa code Rust: build lại web/cleanplate.wasm
```

Ảnh ra giống hệt CLI từng pixel (2528×1696 mất ~9 s, một luồng). Video 720p dài 8 s mất ~21 s, được nén lại bằng
H.264 và giữ nguyên âm thanh; trên bench có ground truth thấp hơn bản AV1 của CLI 0,5 dB (39,03 so với 39,52 dB trên
`night_720p`), do codec chứ không do thuật toán.

## Cách hoạt động

```
video ─► nhận diện (NCC nhiều template, vị trí chuẩn, không chắc thì từ chối)
      ─► tách cảnh (điểm cắt)
      ─► với từng frame, từng pixel của logo:
            đảo lớp phủ trên YUV gốc                    I = (J − a·L) / (1 − a)
            lấy cùng điểm cảnh từ ±K frame               (optical flow + kiểm tra nhất quán)
            trọng số: pixel sạch ≫ phép đảo (1 − a)², × độ tin flow × suy giảm theo thời gian
            loại sai lệch lớn, trung bình bền vững
            MAP–TV có trọng số chỉ ở chỗ không có pixel sạch
      ─► dán lại (viền mềm) vào frame gốc, nén, chép âm thanh + thông tin màu
```

Chi tiết, bằng chứng và các ý tưởng đã thử rồi loại: [`docs/method.md`](./docs/method.md),
[`docs/benchmark.md`](./docs/benchmark.md).

## Giới hạn đã biết

- **Cảnh tĩnh bị giới hạn bởi thông tin.** Nếu nền dưới logo không bao giờ lộ ra, cleanplate trả về hình khả dĩ nhất,
  không phải chi tiết mà bộ nén đã bỏ đi (trường hợp xấu nhất: bếp 9:16, 36.2 so với mức sàn 41.9 dB).
- **Bản tải đã AI upscale** (Flow 1080p/4K) vẽ lại logo theo từng nền; không có `--guide` 720p thì cảnh tĩnh còn một
  vùng mịn rất nhẹ.
- **Logo trên nền trắng tuyệt đối** (chỉ sáng thêm ≤ 7 mức trên nền 253) không tool nào nhận ra, kể cả cleanplate;
  mắt thường cũng gần như không thấy.

## Lộ trình

- [x] Pipeline video tham chiếu + benchmark có đáp án (8 clip)
- [x] Ảnh: tự đo độ phủ, ảnh đã thu nhỏ hoặc nén lại
- [x] Flow 1080p (`--guide`), video app Gemini, ảnh Flow, ảnh qua Zalo
- [x] Lõi Rust + một file CLI (Linux, macOS, Windows) — cùng chất lượng bản tham chiếu, nhanh hơn ~16 lần
- [x] Trang kéo thả chạy trong trình duyệt (WebAssembly + WebCodecs; file không rời khỏi máy bạn)
- [ ] Tuỳ chọn upscale kết quả đã sạch

## Giấy phép

| Phần | Giấy phép |
|---|---|
| Code (`research/`, `tests/`, `docs/*.py`) | [MIT](./LICENSE) |
| Template logo Veo 720p / 1080p (cleanplate tự trích) | MIT |
| Template hình thoi Gemini và chữ Veo cũ | MIT, © Frédéric Guigand, từ [froggeric/gemini-watermark-and-synthid-remover](https://github.com/froggeric/gemini-watermark-and-synthid-remover); xem [ATTRIBUTION](./assets/marks/google/ATTRIBUTION.md) |

Tài liệu tham khảo: Dekel và cộng sự, *On the Effectiveness of Visible Watermarks*, CVPR 2017 · E2FGVI (CVPR 2022) và
ProPainter (ICCV 2023) về lan truyền theo optical flow · DIS optical flow (ECCV 2016) · TV của Rudin–Osher–Fatemi (1992).

Rất hoan nghênh đóng góp: đọc [CONTRIBUTING.md](./CONTRIBUTING.md). Mọi khẳng định "tốt hơn" cần có số liệu từ
`research/score_gt.py` hoặc `research/img_bench.py` kèm ảnh so sánh.
