/**
 * The brand, in one place.
 *
 * Shared so the masthead, the page title and any future share card cannot drift apart.
 * Colours and type are not here — those are CSS custom properties in `style.css`,
 * copied from the Utharam house system, because a design token that lives in two
 * languages is a design token that will disagree.
 */

/** Product name as it appears in the UI. */
export const BRAND = 'LexParser'

/** The mark in the masthead and favicon. One letter, like the Utharam house mark. */
export const MARK = 'L'

/** One line, for a meta description or a README heading. */
export const TAGLINE = 'Legal PDFs into structured JSON, in your browser.'

/** What it does, in a sentence. */
export const DESCRIPTION =
  'Parses Indian Acts and Rules from PDF into Akoma Ntoso JSON — sections, ' +
  'subsections, clauses, citations and footnotes — entirely in the browser.'

/** Attribution. The brand is the credit — an individual name belongs in the suite site. */
export const SUITE = 'Utharam'
export const SUITE_URL = 'https://utharam.in/'

/**
 * The companion tool: a JSON reader for what this one produces.
 *
 * Placeholder while that app is being built. Point `href` at the deployment when it
 * exists — nothing else here needs to change.
 */
export const COMPANION = {
  name: 'StatuteGraph',
  href: 'https://statutegraph.utharam.in/',
  summary:
    'Load the JSON this produces and read it as structure — provision tree, citation ' +
    'graph, and cross-reference trails you can walk in either direction.',
}