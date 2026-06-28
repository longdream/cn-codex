Place bundled OCR runtime assets in this directory:

- `onnxruntime.dll` (Windows ONNX Runtime dynamic library)
- `ppocrv5_mobile_det.onnx` (PP-OCRv5 mobile detection model)
- `ppocrv5_mobile_rec.onnx` (PP-OCRv5 mobile recognition model)
- `ppocrv5_mobile_vocab.txt` (recognition vocabulary, one token per line)

At runtime CN-Codex loads these files for local image OCR fallback/tooling.
