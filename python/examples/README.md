# Examples

Each script takes a compiled workspace: a directory `ekos commit` has run in (it holds `.ekos/`).
To make one from any repository:

```bash
cd your-repo
ekos init --detect && ekos build && ekos recover && ekos resolve && ekos compile && ekos commit
pip install ekos
```

| Script | What it shows |
|---|---|
| [`quickstart.py`](quickstart.py) | open, search, one object with its facts and evidence |
| [`impact_report.py`](impact_report.py) | everything that depends on a table, function or job, hop by hop |
| [`grounded_context.py`](grounded_context.py) | a cited context block for a language model, with disputed claims marked |
| [`schema_overview.py`](schema_overview.py) | every table with columns and references, using EKL and `ekos.aio` |

```bash
python quickstart.py ~/work/shop orders
python impact_report.py ~/work/shop customers 3
python grounded_context.py ~/work/shop "which tables reference customers?"
python schema_overview.py ~/work/shop
```

The guide is [docs/python.md](../../docs/python.md).
