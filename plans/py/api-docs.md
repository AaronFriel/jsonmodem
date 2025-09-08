## Python API Documentation Outputs

The `.agent/check-py.sh` helper now produces two flavours of HTML docs so
contributors can inspect the Python surface quickly without publishing anything.

1. Built-in `pydoc` HTML  
   - Command (already run as part of the check script):

        source .venv/bin/activate
        python -m pydoc -w jsonmodem

   - Output: `tmp/plans/python/pydoc/jsonmodem.html`
   - View locally by opening the HTML file in a browser, for example:

        xdg-open tmp/plans/python/pydoc/jsonmodem.html

2. `pdoc` HTML (richer navigation)  
   - Command (also run automatically):

        source .venv/bin/activate
        uv pip install pdoc
        pdoc -o tmp/plans/python/pdoc jsonmodem

   - Entry point: `tmp/plans/python/pdoc/index.html`
   - Browse via:

        xdg-open tmp/plans/python/pdoc/index.html

Both directories live under `tmp/` so they are ignored by Git. If you rerun only
the documentation steps, ensure the virtualenv is active and the target
directories exist. A quick smoke test looks like:

    source .venv/bin/activate
    mkdir -p tmp/plans/python/{pydoc,pdoc}
    python -m pydoc -w jsonmodem
    mv jsonmodem.html tmp/plans/python/pydoc/
    pdoc -o tmp/plans/python/pdoc jsonmodem

The `pdoc` output tends to be more readable thanks to its sidebar navigation,
but `pydoc` provides a useful baseline without extra dependencies.
