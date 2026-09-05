# EKOS console API (RFC 0128 §3.5). Python only — the `ekos` binary is bind-mounted by Compose,
# not built here.
FROM python:3.12-slim

COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project --no-build
COPY app ./app

# `/app` stays writable by this user: `.ekos-web/` (SQLite registry + run logs, app/models.py)
# is created under the CWD at runtime and isn't a Compose volume, so it lands here.
RUN useradd --create-home --uid 1000 ekos && chown -R ekos:ekos /app
USER ekos

ENV PATH="/app/.venv/bin:$PATH"
EXPOSE 8000
CMD ["uvicorn", "app.main:create_app", "--factory", "--host", "0.0.0.0", "--port", "8000"]
