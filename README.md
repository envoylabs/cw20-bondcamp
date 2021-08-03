![Build and Test](https://github.com/envoylabs/cw20-bondcamp/actions/workflows/build_and_test.yml/badge.svg)

# CW-20 Bondcamp

Excuse the name. More docs coming soon. Issue and buy tokens based on a `Work` by and `Artist` that has a permalink URI.

It is up to the initial artist or curator to supply a stable permalink and image. It may be that this is a link to a page that has an aggregation of different resources for the `Work`.

For example, for a release, you might want a page that aggregates:

- Bandcamp
- Website listing
- Spotify
- Apple Music
- YouTube album stream
- Wikipedia (if a larger artist)

This is intended to be used for discovery, rather than media playback, hence why Wikipedia is potentially a valid reference. Until recently, Tool and MBV weren't on Spotify or online services, but you might want to curate their work.

Alternatively, an artist themselves might want to instantiate one contract _per_ service. Not 100% why you'd do that, but it's an option.

## TODO:

- enforce meta URI uniqueness
  - question: how to handle bad-faith duplicates?
- reward mechanism