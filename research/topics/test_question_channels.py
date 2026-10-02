import unittest

from question_channels import Channel, classify, tokens
from question_html import InvalidHtml, render


class ChannelTests(unittest.TestCase):
    def test_inline_code_stays_prose_and_block_code_separates(self):
        source = '<p>Use <code>tokio::spawn</code> with Rust.</p><pre><code>let x = 1;</code></pre><p>Then await.</p>'
        result = classify(source)
        self.assertEqual(result.rendered, render(source))
        items = [(t.channel, t.term) for t in tokens(result)]
        self.assertIn((Channel.PROSE, 'tokio::spawn'), items)
        self.assertIn((Channel.CODE, 'let'), items)
        self.assertIn((Channel.PROSE, 'await'), items)

    def test_entities_unicode_offsets_and_comments_preserved(self):
        source = '<p>é e\u0301 🦀 &amp; λ</p><!--hidden--><pre>α &lt; β\r\n</pre>'
        result = classify(source)
        self.assertEqual(result.rendered, render(source))
        self.assertIn(Channel.EXCLUDED, result.piece_channels)
        for run in result.runs:
            text = result.rendered.text
            self.assertEqual(text[run.start:run.end].encode(), text.encode()[run.byte_start:run.byte_end])
        self.assertEqual(''.join(result.rendered.text[r.start:r.end] for r in result.runs), result.rendered.text)

    def test_markup_does_not_split_a_contiguous_inline_identifier(self):
        result = classify('<p>foo<code>bar</code>baz</p>')
        self.assertEqual([(t.channel, t.term) for t in tokens(result)], [(Channel.PROSE, 'foobarbaz')])

    def test_empty_selfclosing_and_uppercase_pre_have_explicit_behavior(self):
        self.assertEqual(classify('').runs, ())
        self.assertFalse(tokens(classify('<pre></pre>')))
        self.assertEqual([(t.channel, t.term) for t in tokens(classify('<pre/>after'))], [(Channel.PROSE, 'after')])
        self.assertEqual([(t.channel, t.term) for t in tokens(classify('<PRE>X</PRE>'))], [(Channel.CODE, 'x')])

    def test_pre_in_attribute_or_comment_does_not_change_context(self):
        result = classify('<p title="<pre>">alpha<!-- <pre> --> beta</p>')
        self.assertTrue(all(t.channel == Channel.PROSE for t in tokens(result)))

    def test_source_separator_prevents_cross_channel_token_fabrication(self):
        result = classify('before<pre>middle</pre>after')
        self.assertEqual([t.term for t in tokens(result)], ['before', 'middle', 'after'])
        self.assertEqual([t.channel for t in tokens(result)], [Channel.PROSE, Channel.CODE, Channel.PROSE])

    def test_unsupported_boundaries_active_content_and_caps_fail(self):
        for source in ('<pre>unclosed', '</pre>unmatched', '<pre><pre>x</pre></pre>',
                       '<script>x</script>', '<!DOCTYPE html>', 'x' * 65537):
            with self.subTest(source=source[:25]), self.assertRaises(InvalidHtml):
                classify(source)


if __name__ == '__main__':
    unittest.main()
