# cleanplate — Design

*Ngày: 2026-11-12 · Trạng thái: chờ duyệt*

## 1. Ý định và lý do

Video và ảnh tạo bằng Google Veo/Gemini (qua Gemini app hoặc Google Flow) bị chèn logo nhìn thấy được ở góc dưới phải: hình thoi ✦ của Gemini, chữ "Veo" (bản cũ cỡ lớn và bản mới cỡ nhỏ ~23×10 px). Chủ dự án làm video phi thương mại và muốn một **repo mã nguồn mở, chuyên nghiệp** cho cộng đồng nhà sáng tạo: vừa dùng được ngay, vừa đọc để nghiên cứu.

Tiêu chí số một là **chất lượng**: sau khi xóa không còn viền, vệt, nhấp nháy hay bất kỳ dấu vết nào nhìn thấy được — kể cả khi phóng to và khi video đang chạy. Ưu tiên thứ hai là **phương pháp mới** (đổi mới thật, có số đo chứng minh). Tốc độ và giao diện xếp sau.

Tên `cleanplate` lấy từ thuật ngữ VFX *clean plate* (cảnh quay sạch, không còn lớp phủ) để repo mở rộng được sang logo của bên khác (video marketing, phim…) chứ không gắn chặt với Google.

### Vì sao cần làm (bằng chứng ngày 2026-11-12)

Đo trên video Veo 3.1 Fast thật tạo từ Google Flow (1280×720, 24 fps, 190 frame, chữ "Veo" nhỏ) và video mẫu Gemini diamond (720×1280, 240 frame) của repo froggeric. Chỉ số không cần ảnh gốc (định nghĩa ở §6), **1.0 = không phân biệt được với vùng xung quanh**:

| Video | Tool | residual | flicker | Ghi chú |
|---|---|---|---|---|
| Veo "Veo" nhỏ (thật) | gốc chưa xóa | 19.71 | 0.74 | |
| | wmr 1.17.1 (froggeric, MIT, C++) | 19.76 | 0.73 | **không nhận ra logo, không xóa gì** |
| | wmr `--legacy` | 6.51 | 0.67 | **làm hỏng video**: in thêm chữ "Veo" đen cỡ lớn |
| | GWT Video 0.6.5-demo (allenk, mã đóng) | 4.07 | 0.88 | mắt thường gần sạch, trung bình theo thời gian còn vệt tối |
| Gemini ✦ 48px (mẫu) | gốc | 2.98 | 0.96 | |
| | wmr | 0.79 | 1.06 | lờ mờ viền hình thoi khi tăng tương phản |
| | GWT | 0.79 | 1.02 | bỏ qua 6/240 frame; ô tối trên nền đen |

Hiện trạng hệ sinh thái: công cụ video tốt nhất (allenk) là **mã đóng**; wmr mở nhưng chậm cập nhật logo mới; trên crates.io chỉ có 2 crate xóa logo **ảnh** (dưới 100 lượt tải mỗi crate), chưa có crate video nào.

## 2. Mục tiêu

1. Xóa logo nhìn thấy được của Gemini/Veo trên **ảnh và video**, gồm: Gemini ✦ (36/48/96 px), Veo text cũ (68×30, 99×43), Veo text nhỏ mới (~23×10 ở 720p; cỡ 1080p cần đo).
2. **Thắng cả wmr và GWT** trên bộ benchmark công khai (§6) ở mọi clip, không chỉ trung bình.
3. Ba cách dùng, theo thứ tự ưu tiên: (a) clone repo để nghiên cứu, (b) CLI, (c) web kéo-thả chạy tại máy người dùng (WASM, không upload).
4. Repo đọc được như tài liệu: phương pháp có giải thích toán học, benchmark chạy lại được bằng một lệnh, kết quả có ảnh so sánh.
5. Kiến trúc mở rộng: thêm logo của bên mới = thêm dữ liệu (mask + mô tả vị trí), không sửa code lõi.

## 3. Không làm (non-goals)

- **Không** xóa watermark ẩn (SynthID) hay metadata nguồn gốc AI (C2PA, XMP…). Chỉ xóa cái nhìn thấy được. Đây là quyết định của chủ dự án.
- Không xóa logo/vật thể bất kỳ do người dùng khoanh vùng trong bản đầu (kiến trúc cho phép sau này).
- Không dùng AI sinh ảnh (diffusion, LaMa…) làm phương pháp chính. Lý do: nó bịa nội dung, đi ngược tiêu chí "pixel thật".
- Không có GUI desktop riêng; web WASM thay thế.

## 4. Phản mục tiêu (thất bại dù code "chạy được")

- Còn bất kỳ dấu vết nhìn thấy được: viền, vệt mờ/tối, nhấp nháy giữa các frame.
- **Làm hỏng video khi nhận diện sai** (lỗi wmr `--legacy` ở trên). Quy tắc: khi độ tin cậy thấp, *không sửa gì* và báo rõ cho người dùng.
- Làm giảm chất lượng phần còn lại của video (nén lại quá tay, lệch màu, mất/lệch âm thanh).
- Tuyên bố "tốt hơn" mà không có số đo chạy lại được.

## 5. Ràng buộc

- **Giấy phép MIT.** Được dùng lại mask của wmr (MIT) với ghi công. Không dùng code/binary của allenk VeoWatermarkRemover (mã đóng) — chỉ dùng làm đối thủ benchmark.
- **Ngôn ngữ lõi: Rust** (quyết định của chủ dự án sau khi so với Go). Lõi phải biên dịch được sang `wasm32` → lõi không được phụ thuộc IO/ffmpeg/OpenCV.
- **Nghiên cứu bằng Python trước** (numpy/OpenCV): thuật toán chỉ được port sang Rust khi đã thắng trên benchmark.
- ffmpeg: máy dev hiện có ffmpeg 8.1 **không có libx264** (Fedora). CLI phải tự chọn encoder có sẵn hoặc dùng ffmpeg tĩnh tải về (vd. `ffmpeg-sidecar`). Không được giả định có libx264.
- Bộ nhớ: không nạp toàn bộ video float vào RAM (đã từng làm OOM máy 15 GB). Xử lý theo luồng, chỉ giữ vùng quanh logo cho các bước cần nhiều frame.

## 6. Benchmark và tiêu chí chấp nhận

**Chỉ số không cần ảnh gốc** (video thật):
- `residual` = trong vùng logo, độ lớn thành phần tần số cao của *ảnh trung bình theo thời gian*, chia cho cùng đại lượng ở vành xung quanh. Logo đứng yên còn nền di chuyển, nên dấu vết sót lại hiện rõ trong ảnh trung bình.
- `flicker` = độ thay đổi giữa hai frame liên tiếp trong vùng logo, chia cho vành xung quanh.
- Ảnh ghép so sánh (phóng 4×, 3 frame + ảnh trung bình tăng tương phản) cho mỗi clip.

Bản nháp của các chỉ số này đã có ở `../eval.py`.

**Chỉ số có ảnh gốc** (dữ liệu tổng hợp): chèn logo bằng đúng mô hình alpha lên video sạch, nén H.264 ở bitrate kiểu Veo (~6.5 Mb/s ở 720p), rồi đo PSNR/SSIM trong vùng logo so với bản gốc.

**Bộ dữ liệu:** video Veo tham chiếu đã tải (`ref/veo31fast_flower_720p.mp4`), 2 video mẫu trong repo froggeric, cộng thêm clip mới đa dạng (nền tối, nền trắng cháy, chuyển động nhanh, 9:16, 1080p) — số lượng do builder quyết.

**Chấp nhận v1** (ngưỡng là đề xuất của mình, chưa phải của chủ dự án):
- Mọi clip thật: `residual ≤ 1.2` và `flicker ≤ 1.1`, và tốt hơn cả wmr lẫn GWT trên từng clip.
- Dữ liệu tổng hợp: PSNR vùng logo cao hơn wmr và GWT.
- Không clip nào bị sửa khi không có logo (kiểm tra bằng clip sạch).

## 7. Phương pháp (đã quyết định ở mức hướng đi)

**Mô hình:** `J = (1 − α)·I + α·L` (L ≈ trắng). Phục hồi: `I = (J − α·L) / (1 − α)`. Đây là nền tảng chung với wmr/GWT; cái mới nằm ở cách có được α chính xác và xử lý chỗ mà phép nghịch đảo không đủ.

Ba hướng đổi mới cần chứng minh bằng benchmark (thứ tự ưu tiên):

1. **Ước lượng α riêng cho từng video từ nhiều frame.** Logo đứng yên, nền chuyển động → có hàng trăm phương trình cho cùng một α(p). Giải α theo từng pixel (khởi tạo bằng mask chuẩn, tinh chỉnh có ràng buộc), bao gồm cả lệch dưới pixel và ảnh hưởng của nén. Tham khảo: Dekel et al., *On the Effectiveness of Visible Watermarks*, CVPR 2017. Lưu ý từ thử nghiệm: chỉ fit 3 tham số (dx, dy, gain) trên mask chuẩn đã **thua** wmr — phải giải α theo pixel, không phải chỉnh tham số.
2. **Lấp theo thời gian cho pixel không phục hồi được.** Pixel cháy trắng (J ≈ 255) hoặc α quá cao thì mất thông tin. Lấy nền thật từ frame lân cận (bù chuyển động) thay vì inpaint không gian như TELEA. Chỉ fallback sang inpaint không gian khi không frame nào có dữ liệu.
3. **Ổn định theo thời gian.** Hiệu chỉnh nhất quán giữa các frame để không nhấp nháy.

**Xử lý trong không gian màu gốc của video** (đề xuất của mình): video là YUV 4:2:0; nên cân nhắc nghịch đảo trên mặt phẳng Y/U/V thay vì đổi qua lại RGB, để tránh mất mát thêm. Builder kiểm chứng bằng benchmark trước khi chốt.

**Nhận diện an toàn:** NCC trên ảnh trung bình (đã lọc thông cao) của nhiều frame, so với danh mục template. Dưới ngưỡng tin cậy → không sửa, báo lỗi rõ.

**Ảnh tĩnh:** chỉ có 1 frame, nên dùng mask chuẩn + đăng ký dưới pixel + fit gain, và chỉ inpaint không gian ở pixel cháy.

**Danh mục logo dạng dữ liệu:** mỗi logo = mask PNG (alpha × 255) + file mô tả (vendor, model, cỡ, lề tới góc theo độ phân giải, màu L). Thêm vendor mới không cần sửa code.

## 8. Kiến trúc và cấu trúc repo

Cargo workspace:

```
cleanplate/
  crates/
    cleanplate-core/   # thuần tính toán: mô hình, detect, ước lượng α, phục hồi; no_std-friendly, không IO
    cleanplate-cli/    # đọc/ghi ảnh-video (ffmpeg pipe), batch, chọn encoder
    cleanplate-wasm/   # (giai đoạn sau) binding cho web
  web/                 # (giai đoạn sau) trang kéo-thả tĩnh dùng WASM
  assets/marks/        # danh mục logo: <vendor>/<name>.png + .toml
  research/            # Python: eval, prototype thuật toán, notebook kết quả
  bench/               # script tải/tạo dữ liệu, chạy benchmark, sinh báo cáo + ảnh so sánh
  docs/                # method.md (toán), benchmark.md (kết quả), adding-a-mark.md
  README.md            # song ngữ EN/VI, ảnh trước/sau, kết quả benchmark
  LICENSE              # MIT + ghi công (wmr masks, Dekel et al.)
```

**Đầu ra video:** chép nguyên âm thanh; giữ metadata màu (primaries, transfer, range); mặc định nén chất lượng cao theo bitrate/codec nguồn, có tùy chọn lossless (FFV1) cho người hậu kỳ.

**CI (GitHub Actions):** fmt, clippy, test, benchmark rút gọn trên clip nhỏ; phát hành binary Linux/macOS/Windows.

## 9. Lộ trình

| Giai đoạn | Nội dung | Kết thúc khi |
|---|---|---|
| P0 | Khung benchmark: chỉ số, bộ dữ liệu, dữ liệu tổng hợp, báo cáo tự động. Baseline wmr/GWT | Chạy 1 lệnh ra bảng + ảnh so sánh |
| P1 | Lõi Rust: mô hình, danh mục logo, detect an toàn, phục hồi cơ bản; CLI ảnh + video | Ngang GWT trên clip Veo nhỏ, không làm hỏng clip sạch |
| P2 | Nghiên cứu (Python) rồi port: α theo video, lấp theo thời gian, ổn định | Đạt tiêu chí §6 |
| P3 | Ảnh tĩnh đạt cùng chuẩn | Benchmark ảnh |
| P4 | Đóng gói public: README song ngữ, docs, CI, release | Repo sẵn sàng public |
| P5 | Web kéo-thả WASM | Chạy trong trình duyệt, không upload |

## 10. Để lại cho builder

Thuật toán cụ thể để giải α (least squares, IRLS, gradient-domain…), cách bù chuyển động, chọn crate (image, ndarray, rayon…), ngưỡng nội bộ, định dạng file mô tả logo, cách lưu dữ liệu benchmark lớn (Git LFS hay script tải), cách đặt tên lệnh CLI.

## 11. Các lựa chọn là của mình (chủ dự án có thể bác)

- Ngưỡng chấp nhận ở §6.
- Không dùng AI sinh ảnh, kể cả làm fallback.
- Xử lý trong YUV gốc (chờ benchmark xác nhận).
- README song ngữ EN/VI.
- Danh mục logo dạng dữ liệu.
- Web làm sau cùng.
- Tốc độ: chất lượng thắng tốc độ, nhưng chế độ mặc định không chậm quá ~10× thời lượng clip trên CPU 12 nhân (8 s clip ≤ ~80 s). Có cờ `--fast` dùng bản nghịch đảo cơ bản.
- Video sạch cho dữ liệu tổng hợp lấy từ nguồn giấy phép mở (vd. phim mở của Blender, CC-BY), ghi nguồn trong `bench/`.
- Encoder mặc định: cùng codec với nguồn nếu ffmpeg có encoder đó (ưu tiên libx264/libx265), không có thì AV1 (SVT-AV1) chất lượng cao, và luôn có `--lossless` (FFV1).
- `../eval.py` và `../proto.py` chuyển vào `research/` làm điểm xuất phát P0.
