"""The error shared by every delivery transport."""


class DeliveryError(RuntimeError):
    """Delivery failed; the saved batch remains available for recovery."""

    def __init__(self, message: str, *, retryable: bool) -> None:
        super().__init__(message)
        self.retryable = retryable
