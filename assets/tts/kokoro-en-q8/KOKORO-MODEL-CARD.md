---
license: apache-2.0
library_name: transformers.js
language:
- en
base_model:
- hexgrad/Kokoro-82M
pipeline_tag: text-to-speech
---

# Kokoro TTS

Kokoro is a frontier TTS model for its size of 82 million parameters (text in/audio out).

## Table of contents

- [Usage](#usage)
  - [JavaScript](#javascript)
  - [Python](#python)
- [Voices/Samples](#voicessamples)
- [Quantizations](#quantizations)


## Usage

### JavaScript

First, install the `kokoro-js` library from [NPM](https://npmjs.com/package/kokoro-js) using:
```bash
npm i kokoro-js
```

You can then generate speech as follows:

```js
import { KokoroTTS } from "kokoro-js";

const model_id = "onnx-community/Kokoro-82M-ONNX";
const tts = await KokoroTTS.from_pretrained(model_id, {
  dtype: "q8", // Options: "fp32", "fp16", "q8", "q4", "q4f16"
});

const text = "Life is like a box of chocolates. You never know what you're gonna get.";
const audio = await tts.generate(text, {
  // Use `tts.list_voices()` to list all available voices
  voice: "af_bella",
});
audio.save("audio.wav");
```


### Python

```python
import os
import numpy as np
from onnxruntime import InferenceSession

# You can generate token ids as follows:
#   1. Convert input text to phonemes using https://github.com/hexgrad/misaki
#   2. Map phonemes to ids using https://huggingface.co/hexgrad/Kokoro-82M/blob/785407d1adfa7ae8fbef8ffd85f34ca127da3039/config.json#L34-L148
tokens = [50, 157, 43, 135, 16, 53, 135, 46, 16, 43, 102, 16, 56, 156, 57, 135, 6, 16, 102, 62, 61, 16, 70, 56, 16, 138, 56, 156, 72, 56, 61, 85, 123, 83, 44, 83, 54, 16, 53, 65, 156, 86, 61, 62, 131, 83, 56, 4, 16, 54, 156, 43, 102, 53, 16, 156, 72, 61, 53, 102, 112, 16, 70, 56, 16, 138, 56, 44, 156, 76, 158, 123, 56, 16, 62, 131, 156, 43, 102, 54, 46, 16, 102, 48, 16, 81, 47, 102, 54, 16, 54, 156, 51, 158, 46, 16, 70, 16, 92, 156, 135, 46, 16, 54, 156, 43, 102, 48, 4, 16, 81, 47, 102, 16, 50, 156, 72, 64, 83, 56, 62, 16, 156, 51, 158, 64, 83, 56, 16, 44, 157, 102, 56, 16, 44, 156, 76, 158, 123, 56, 4]

# Context length is 512, but leave room for the pad token 0 at the start & end
assert len(tokens) <= 510, len(tokens)

# Style vector based on len(tokens), ref_s has shape (1, 256)
voices = np.fromfile('./voices/af.bin', dtype=np.float32).reshape(-1, 1, 256)
ref_s = voices[len(tokens)]

# Add the pad ids, and reshape tokens, should now have shape (1, <=512)
tokens = [[0, *tokens, 0]]

model_name = 'model.onnx' # Options: model.onnx, model_fp16.onnx, model_quantized.onnx, model_q8f16.onnx, model_uint8.onnx, model_uint8f16.onnx, model_q4.onnx, model_q4f16.onnx
sess = InferenceSession(os.path.join('onnx', model_name))

audio = sess.run(None, dict(
    input_ids=tokens,
    style=ref_s,
    speed=np.ones(1, dtype=np.float32),
))[0]
```

Optionally, save the audio to a file:
```py
import scipy.io.wavfile as wavfile
wavfile.write('audio.wav', 24000, audio[0])
```


## Voices/Samples


> Life is like a box of chocolates. You never know what you're gonna get.


| Name         | Nationality | Gender | Sample                                                                                                                                  |
| ------------ | ----------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| **af_heart** | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/S_9tkA75BT_QHKOzSX6S-.wav"></audio> |
| af_alloy     | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/wiZ3gvlL--p5pRItO4YRE.wav"></audio> |
| af_aoede     | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/Nv1xMwzjTdF9MR8v0oEEJ.wav"></audio> |
| af_bella     | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/sWN0rnKU6TlLsVdGqRktF.wav"></audio> |
| af_jessica   | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/2Oa4wITWAmiCXJ_Q97-7R.wav"></audio> |
| af_kore      | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/AOIgyspzZWDGpn7oQgwtu.wav"></audio> |
| af_nicole    | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/EY_V2OGr-hzmtTGrTCTyf.wav"></audio> |
| af_nova      | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/X-xdEkx3GPlQG5DK8Gsqd.wav"></audio> |
| af_river     | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/ZqaV2-xGUZdBQmZAF1Xqy.wav"></audio> |
| af_sarah     | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/xzoJBl1HCvkE8Fl8Xu2R4.wav"></audio> |
| af_sky       | American    | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/ubebYQoaseyQk-jDLeWX7.wav"></audio> |
| am_adam      | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/tvauhDVRGvGK98I-4wv3H.wav"></audio> |
| am_echo      | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/qy_KuUB0hXsu-u8XaJJ_Z.wav"></audio> |
| am_eric      | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/JhqPjbpMhraUv5nTSPpwD.wav"></audio> |
| am_fenrir    | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/c0R9caBdBiNjGUUalI_DQ.wav"></audio> |
| am_liam      | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/DFHvulaLeOjXIDKecvNG3.wav"></audio> |
| am_michael   | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/IPKhsnjq1tPh3JmHH8nEg.wav"></audio> |
| am_onyx      | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/ov0pFDfE8NNKZ80LqW6Di.wav"></audio> |
| am_puck      | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/MOC654sLMHWI64g8HWesV.wav"></audio> |
| am_santa     | American    | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/LzA6JmHBvQlhOviy8qVfJ.wav"></audio> |
| bf_alice    | British     | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/9mnYZ3JWq7f6U12plXilA.wav"></audio> |
| bf_emma     | British     | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/_fvGtKMttRI0cZVGqxMh8.wav"></audio> |
| bf_isabella | British     | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/VzlcJpqGEND_Q3duYnhiu.wav"></audio> |
| bf_lily     | British     | Female | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/qZCoartohiRlVamY8Xpok.wav"></audio> |
| bm_daniel   | British     | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/Eb0TLnLXHDRYOA3TJQKq3.wav"></audio> |
| bm_fable    | British     | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/NT9XkmvlezQ0FJ6Th5hoZ.wav"></audio> |
| bm_george   | British     | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/y6VJbCESszLZGupPoqNkF.wav"></audio> |
| bm_lewis    | British     | Male   | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/RlB5BRvLt-IFvTjzQNxCh.wav"></audio> |


## Quantizations

The model is resilient to quantization, enabling efficient high-quality speech synthesis at a fraction of the original model size. 

> How could I know? It's an unanswerable question. Like asking an unborn child if they'll lead a good life. They haven't even been born.


| Model                                          | Size (MB) | Sample                                                                                                                                  |
|------------------------------------------------|-----------|-----------------------------------------------------------------------------------------------------------------------------------------|
| model.onnx (fp32)                              | 326       | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/njexBuqPzfYUvWgs9eQ-_.wav"></audio> |
| model_fp16.onnx (fp16)                         | 163       | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/8Ebl44hMQonZs4MlykExt.wav"></audio> |
| model_quantized.onnx (8-bit)                   | 92.4      | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/9SLOt6ETclZ4yRdlJ0VIj.wav"></audio> |
| model_q8f16.onnx (Mixed precision)             | 86        | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/gNDMqb33YEmYMbAIv_Grx.wav"></audio> |
| model_uint8.onnx (8-bit & mixed precision)     | 177       | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/tpOWRHIWwEb0PJX46dCWQ.wav"></audio> |
| model_uint8f16.onnx (Mixed precision)          | 114       | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/vtZhABzjP0pvGD7dRb5Vr.wav"></audio> |
| model_q4.onnx (4-bit matmul)                   | 305       | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/8FVn0IJIUfccEBWq8Fnw_.wav"></audio> |
| model_q4f16.onnx (4-bit matmul & fp16 weights) | 154       | <audio controls src="https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/7DrgWC_1q00s-wUJuG44X.wav"></audio> |
