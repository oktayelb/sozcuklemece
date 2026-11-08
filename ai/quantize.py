import sys

import numpy
import onnx
from onnxruntime.quantization import (
    CalibrationDataReader,
    CalibrationMethod,
    QuantFormat,
    QuantType,
    quantize_static,
)
from onnxruntime.quantization.shape_inference import quant_pre_process
from tokenizers import Tokenizer

MAX_TOKENS = 75
PREPARED_MODEL = "/tmp/prepared_model.onnx"


def read_texts(texts_path):
    texts = []
    with open(texts_path, encoding="utf-8") as file:
        for line in file:
            if line.strip():
                texts.append(line.strip("\n"))
    return texts


def matmuls_without_weights(model):
    weight_names = set()
    for weight in model.graph.initializer:
        weight_names.add(weight.name)

    names = []
    for node in model.graph.node:
        if node.op_type != "MatMul":
            continue
        uses_a_weight = False
        for input_name in node.input:
            if input_name in weight_names:
                uses_a_weight = True
        if not uses_a_weight:
            names.append(node.name)
    return names


class CalibrationTexts(CalibrationDataReader):
    def __init__(self, texts, tokenizer, input_names):
        self.texts = texts
        self.tokenizer = tokenizer
        self.input_names = input_names
        self.next_index = 0

    def get_next(self):
        if self.next_index >= len(self.texts):
            return None
        text = self.texts[self.next_index]
        self.next_index += 1
        if "\t" in text:
            first_text, second_text = text.split("\t", 1)
            encoding = self.tokenizer.encode(first_text, second_text)
        else:
            encoding = self.tokenizer.encode(text)

        values = {
            "input_ids": encoding.ids,
            "attention_mask": encoding.attention_mask,
            "token_type_ids": encoding.type_ids,
        }
        feed = {}
        for name in self.input_names:
            feed[name] = numpy.array([values[name]], dtype=numpy.int64)
        return feed


def main():
    source_model, quantized_model, tokenizer_path, texts_path = sys.argv[1:5]

    quant_pre_process(source_model, PREPARED_MODEL, skip_symbolic_shape=True)
    prepared = onnx.load(PREPARED_MODEL)
    input_names = []
    for graph_input in prepared.graph.input:
        input_names.append(graph_input.name)

    tokenizer = Tokenizer.from_file(tokenizer_path)
    tokenizer.enable_truncation(MAX_TOKENS)
    calibration = CalibrationTexts(read_texts(texts_path), tokenizer, input_names)

    quantize_static(
        PREPARED_MODEL,
        quantized_model,
        calibration,
        quant_format=QuantFormat.QDQ,
        per_channel=True,
        activation_type=QuantType.QUInt8,
        weight_type=QuantType.QInt8,
        op_types_to_quantize=["MatMul"],
        nodes_to_exclude=matmuls_without_weights(prepared),
        calibrate_method=CalibrationMethod.MinMax,
    )


main()
