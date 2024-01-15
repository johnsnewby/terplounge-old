# Solotandem (or some other name)

This repository contains the various components of the project which is sometimes called 'Terplounge'. It consists of the following components:



# Installation

## Installation steps

In order to run this, you will need a whisper model--currently hardcoded to 'medium'. Download it like this:

```
cd server
../scripts/download-ggml-model.sh [model_name]
```

These are the models available:

```
[
    "tiny.en", "tiny", "tiny-q5_1", "tiny.en-q5_1",
    "base.en", "base", "base-q5_1", "base.en-q5_1",
    "small.en", "small.en-tdrz", "small", "small-q5_1",
    "small.en-q5_1", "medium", "medium.en", "medium-q5_0",
    "medium.en-q5_0",  "large-v1", "large", "large-q5_0"
]
```

run the server from the `server` directory:

```
cargo run
```

## Environment variables100

```
WHISPER_THREADS=
LISTEN=
WHISPER_MODEL=
RUST_LOG=
RUST_BACKTRACE=
```

## Testing

open the file `websocket.html` in your browser, and hit start recording. If you are lucky you'll get a couple of seconds of transcription.
