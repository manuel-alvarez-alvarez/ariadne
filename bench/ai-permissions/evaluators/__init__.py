"""Every evaluator the runner can run, one package per backend.

`kev/` and `laya/` each hold a backend base class, which loads and releases the model in
`setup` and `teardown`, and one module per mode, whose concrete class implements `evaluate`
and registers under its own key (`ai_bench.registry.register`).
"""
