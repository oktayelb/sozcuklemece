FROM --platform=$BUILDPLATFORM python:3.12-slim AS export

ARG MODEL_ID=emrecan/bert-base-turkish-cased-allnli_tr
ARG MODEL_REVISION=c71182e80ce1bba21d07d1f1dd18ebef5228b0b6

RUN pip install --no-cache-dir \
    --extra-index-url https://download.pytorch.org/whl/cpu \
    torch==2.15.0+cpu \
    optimum==2.1.0 \
    optimum-onnx==0.1.0 \
    transformers==4.57.6 \
    onnx==1.23.2 \
    onnxruntime==1.31.1 \
    huggingface_hub==0.36.2

RUN hf download "$MODEL_ID" --revision "$MODEL_REVISION" --local-dir /source \
    --exclude ".gitattributes" "README.md" "training_args.bin"
COPY quantize.py nli_calibration_texts.txt /quantization/
RUN optimum-cli export onnx --model /source --task text-classification /exported \
    && mkdir -p /model/onnx \
    && python /quantization/quantize.py /exported/model.onnx /model/onnx/model.onnx \
        /exported/tokenizer.json /quantization/nli_calibration_texts.txt \
    && cp /exported/config.json /exported/tokenizer.json /exported/tokenizer_config.json \
        /exported/special_tokens_map.json /model/ \
    && rm -rf /source /exported /tmp/prepared_model.onnx

FROM rust:1.96-trixie AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
    && cp target/release/sozcuklemece-ai /sozcuklemece-ai

FROM gcr.io/distroless/cc-debian13:nonroot

COPY --from=export /model /model
COPY --from=build /sozcuklemece-ai /usr/local/bin/sozcuklemece-ai

ENTRYPOINT ["/usr/local/bin/sozcuklemece-ai"]
CMD ["classify", "/model"]
