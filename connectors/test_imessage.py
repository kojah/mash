"""Mapping tests for the iMessage connector, runnable without Messages access."""

import unittest
from datetime import UTC, datetime

from connectors.blueprints.imessage import Conversation, Post, chat, message
from connectors.common import Sender


def post(sender: Sender | None = None) -> Post:
    return Post(
        id="m1",
        chat_id="c1",
        content="hello",
        sender=sender,
        created_at=datetime(2026, 1, 1, tzinfo=UTC),
        updated_at=None,
        files=[],
    )


class MappingTests(unittest.TestCase):
    def test_entry_always_carries_an_author(self) -> None:
        # Sender-less rows are still evidence; the model requires an author on
        # every entry, so they are attributed to nobody explicitly.
        entry = message(post())
        self.assertIsNotNone(entry.author)
        self.assertEqual(entry.author.id, "unknown")
        self.assertEqual(entry.author.kind, "unknown")

    def test_sender_maps_to_the_author(self) -> None:
        entry = message(post(Sender(id="U1", name="Alice", kind="human")))
        self.assertEqual(entry.author.id, "U1")
        self.assertEqual(entry.author.name, "Alice")
        self.assertEqual(entry.author.kind, "human")

    def test_chat_is_a_container_and_the_entry_parents_it(self) -> None:
        container = chat(Conversation(id="c1", title="Chat", participants=[]))
        self.assertEqual(container.kind, "imessage:chat")
        self.assertEqual(container.title, "Chat")
        self.assertEqual(message(post()).parent, "c1")
