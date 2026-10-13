"""Acceptance consumes final installed bytes, never pre-fixup build output."""
import hashlib


def verify_final_plugin(path, expected):
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != expected:
        raise ValueError("FINAL installed plugin digest mismatch")
    return actual
