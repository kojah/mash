"""Session recall for the Mash plugin: project-scoped recent chats and records.

These modules run inside the agent host's own `python3`, outside any managed
environment, so they use only the standard library (`json`, `argparse`,
`dataclasses`) and stay importable on Python 3.9.
"""
