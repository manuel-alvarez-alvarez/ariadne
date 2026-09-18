"""The Python fixture."""

LIMIT = 3


def clamp(value):
    """Keeps a value under the limit."""
    return min(value, LIMIT)


class Gauge:
    """Reads a value."""

    def read(self):
        """Reads it."""
        return clamp(4)


def test_clamp():
    assert clamp(5) == 3
