# Test clips

From LibriSpeech `dev-clean` (speaker 1272), via the `hf-internal-testing/librispeech_asr_dummy` sample on Hugging Face, converted to 16 kHz mono WAV, the format `lemonade-relay` sends to Lemonade.

- `short.wav` (5.9 s): utterance `1272-128104-0000`
- `long.wav` (29.4 s): utterance `1272-128104-0004`
- `acc-00.wav` to `acc-19.wav` (118 s in total): utterances `1272-141231-0000` to `-0019`, used once per model to compare accuracy

Reference texts are in `refs.json`.

LibriSpeech is by Vassil Panayotov, Guoguo Chen, Daniel Povey and Sanjeev Khudanpur, licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Source: <https://www.openslr.org/12>.
