FROM --platform=$BUILDPLATFORM python:3.12-slim AS export

ARG MODEL_ID=emrecan/bert-base-turkish-cased-mean-nli-stsb-tr
ARG MODEL_REVISION=7e10e2fbd7627fd0228eb2dbdc9e9da264442bbe

RUN pip install --no-cache-dir \
    --extra-index-url https://download.pytorch.org/whl/cpu \
    torch==2.15.0+cpu \
    optimum==2.1.0 \
    optimum-onnx==0.1.0 \
    transformers==4.57.6 \
    onnx==1.23.2 \
    onnxruntime==1.31.1 \
    huggingface_hub==0.36.2

RUN hf download "$MODEL_ID" --revision "$MODEL_REVISION" --local-dir /model \
    --exclude "openvino/*" "pytorch_model.bin" ".gitattributes" "README.md"
COPY quantize.py calibration_texts.txt /quantization/
RUN optimum-cli export onnx --model /model --task feature-extraction /exported \
    && mkdir /model/onnx \
    && python /quantization/quantize.py /exported/model.onnx /model/onnx/model.onnx \
        /model/tokenizer.json /quantization/calibration_texts.txt \
    && rm -rf /model/model.safetensors /model/.cache /exported /tmp/prepared_model.onnx

FROM rust:1.96-trixie AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=ai-target,target=/build/target \
    cargo build --release --locked \
    && cp target/release/sozcuklemece-ai /sozcuklemece-ai

FROM gcr.io/distroless/cc-debian13:nonroot

COPY --from=export /model /model
COPY --from=build /sozcuklemece-ai /usr/local/bin/sozcuklemece-ai

ENTRYPOINT ["/usr/local/bin/sozcuklemece-ai"]
CMD ["embed", "/model"]
