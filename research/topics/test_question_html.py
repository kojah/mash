import unittest

from question_html import InvalidHtml, Kind, render
from reference_review import digest


class HtmlTests(unittest.TestCase):
    def test_code_entities_and_block_separators_keep_original_words(self):
        source = '<p>Hello <em>rust</em> &amp; C++.</p><pre><code>a &lt; b\n🦀</code></pre>'
        result = render(source)
        self.assertEqual(result.text, 'Hello rust & C++.\na < b\n🦀\n')
        self.assertEqual(result.source_sha256, digest(source.encode()))
        for piece in result.pieces:
            self.assertEqual(source[piece.start:piece.end].encode(), source.encode()[piece.byte_start:piece.byte_end])
            self.assertEqual(piece.rendered_sha256, digest(result.text[piece.rendered_start:piece.rendered_end].encode()))
            self.assertEqual(result.text[piece.rendered_start:piece.rendered_end].encode(),
                             result.text.encode()[piece.rendered_byte_start:piece.rendered_byte_end])

    def test_crlf_unicode_combining_marks_and_numeric_entities_have_byte_coordinates(self):
        source = '<p>é\r\ne\u0301 &#x1F980; &#169;</p>'
        result = render(source)
        self.assertEqual(result.text, 'é\r\ne\u0301 🦀 ©\n')
        self.assertTrue(any(p.byte_start != p.start for p in result.pieces))
        self.assertEqual(sum(p.end-p.start for p in result.pieces if p.kind != Kind.SEPARATOR), len(source))

    def test_attributes_comments_and_empty_markup_are_explicit_exclusions(self):
        source = '<p>Read <a href="https://example.org?a=>">this</a><!--hidden--><br/>next.</p>'
        result = render(source)
        self.assertEqual(result.text, 'Read this\nnext.\n')
        self.assertEqual(sum(p.kind == Kind.COMMENT for p in result.pieces), 1)
        self.assertNotIn('example', result.text)
        self.assertNotIn('hidden', result.text)
        self.assertEqual(render('<img src="image.png"/>').text, '')

    def test_active_layout_declarations_and_caps_fail_without_partial_success(self):
        for source in ['<script>content</script>', '<style>content</style>', '<!DOCTYPE html>',
                       '<?xml version="1"?>', 'x' * 65537, '<p>x</p>' * 1500]:
            with self.subTest(source=source[:30]), self.assertRaises(InvalidHtml):
                render(source)

    def test_unparsed_entity_suffix_is_preserved_as_literal_data(self):
        self.assertEqual(render('&amp').text, '&amp')
        self.assertEqual(render('&mashunknown;').text, '&mashunknown;')


if __name__ == '__main__':
    unittest.main()
