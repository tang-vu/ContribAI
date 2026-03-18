# ── Build ──────────────────────────────────────────
FROM python:3.12-slim AS builder

WORKDIR /build
COPY pyproject.toml .
COPY contribai/ contribai/
COPY README.md .
COPY LICENSE .

RUN pip install --no-cache-dir build && \
    python -m build --wheel

# ── Runtime ───────────────────────────────────────
FROM python:3.12-slim

LABEL maintainer="ContribAI Team"
LABEL description="AI Agent for Open Source Contributions"

# Create non-root user
RUN useradd --create-home --shell /bin/bash contribai
WORKDIR /home/contribai

# Install the built wheel
COPY --from=builder /build/dist/*.whl /tmp/
RUN pip install --no-cache-dir /tmp/*.whl && rm /tmp/*.whl

# Create config and data directories
RUN mkdir -p /home/contribai/.contribai && \
    chown -R contribai:contribai /home/contribai

# Expose dashboard port
EXPOSE 8787

USER contribai

# Health check for dashboard mode
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD python -c "import httpx; httpx.get('http://localhost:8787/api/health')" || exit 1

# Default: show help
ENTRYPOINT ["contribai"]
CMD ["--help"]
