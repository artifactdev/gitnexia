import type { ThemeRegistration } from 'shiki';

/**
 * Bearded Theme's Arc Blueberry as a TextMate theme for Shiki/Monaco. Shiki doesn't ship it; the
 * colours are the theme's own palette and Zed port (BeardedBear/bearded-theme, GPL-3.0), used
 * here as a colour scheme (the app theme in themes.ts is GitNexia's own data). The theme's base is
 * #111422 with #8eb0e6 as its primary, its syntax colours from the same palette.
 */
export const beardedArcBlueberry: ThemeRegistration = {
  name: 'bearded-arc-blueberry',
  displayName: 'Bearded Arc Blueberry',
  type: 'dark',
  colors: {
    'editor.background': '#111422',
    'editor.foreground': '#bcc1dc',
    'editorLineNumber.foreground': '#44508880',
    'editorLineNumber.activeForeground': '#6673b3',
    'editorGutter.background': '#111422',
    'editor.selectionBackground': '#8eb0e640',
    'editor.inactiveSelectionBackground': '#8eb0e640',
    'editor.lineHighlightBackground': '#1a1e334d',
    'editorCursor.foreground': '#8eb0e6',
    'editorWhitespace.foreground': '#bcc1dc26',
    'editorIndentGuide.background1': '#bcc1dc1a',
    'editorIndentGuide.activeBackground1': '#bcc1dc33',
    'editor.findMatchBackground': '#eacd614d',
    'editor.findMatchHighlightBackground': '#eacd6133',
    'scrollbarSlider.background': '#bcc1dc26',
    'scrollbarSlider.hoverBackground': '#bcc1dc40',
    'minimap.background': '#111422',
  },
  tokenColors: [
    { scope: ['comment', 'punctuation.definition.comment'], settings: { foreground: '#3c4776', fontStyle: 'italic' } },
    { scope: ['string', 'string.quoted', 'punctuation.definition.string'], settings: { foreground: '#3cec85' } },
    { scope: ['constant.character.escape', 'constant.other.placeholder', 'string.regexp', 'string.special'], settings: { foreground: '#ff955c' } },
    { scope: ['constant.numeric'], settings: { foreground: '#e35535' } },
    { scope: ['constant.language', 'constant.character', 'constant.other'], settings: { foreground: '#e35535' } },
    { scope: ['keyword', 'keyword.control', 'keyword.operator.new', 'keyword.other', 'storage', 'storage.modifier'], settings: { foreground: '#eacd61' } },
    { scope: ['keyword.function', 'storage.type'], settings: { foreground: '#22ecdb' } },
    { scope: ['keyword.operator', 'operator'], settings: { foreground: '#eacd61' } },
    { scope: ['entity.name.function', 'support.function', 'meta.function-call entity.name.function'], settings: { foreground: '#69c3ff' } },
    { scope: ['entity.name.type', 'support.type', 'support.class', 'entity.other.inherited-class'], settings: { foreground: '#b78aff' } },
    { scope: ['entity.name.class', 'entity.name.namespace'], settings: { foreground: '#a4ef58' } },
    { scope: ['variable', 'variable.other', 'variable.other.readwrite'], settings: { foreground: '#ff738a' } },
    { scope: ['variable.parameter'], settings: { foreground: '#f38cec' } },
    { scope: ['variable.other.property', 'variable.other.object.property', 'support.variable.property', 'meta.property-name', 'support.type.property-name'], settings: { foreground: '#bcc1dc' } },
    { scope: ['entity.name.tag'], settings: { foreground: '#69c3ff' } },
    { scope: ['entity.other.attribute-name'], settings: { foreground: '#eacd61' } },
    { scope: ['punctuation', 'punctuation.bracket', 'punctuation.delimiter'], settings: { foreground: '#bcc1dc66' } },
    { scope: ['markup.heading', 'title'], settings: { foreground: '#eacd61', fontStyle: 'bold' } },
    { scope: ['markup.bold'], settings: { foreground: '#ff738a', fontStyle: 'bold' } },
    { scope: ['markup.italic'], settings: { foreground: '#ff955c', fontStyle: 'italic' } },
    { scope: ['markup.inserted'], settings: { foreground: '#3cec85' } },
    { scope: ['markup.deleted', 'invalid'], settings: { foreground: '#e35535' } },
    { scope: ['text.literal', 'string.other.link'], settings: { foreground: '#b78aff' } },
  ],
};
