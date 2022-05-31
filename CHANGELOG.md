# Changelog
All notable changes to this project will be documented in this file.

<!--
### Changed
- -->
## [0.2.4] - 2021-07-22
- (#10) Add browsing token scopes

## [0.2.4] - 2021-07-22
- (#7) Add additional token scope
- Bump dependencies 

## [0.2.3] - 2021-06-04
- Add `x86_64` to CI
- Bump dependencies 

## [0.2.2] - 2020-10-07
- Fix Android/iOS track skipping display issues 
- Set an explicit period size to reduce CUP load

## [0.2.1] - 2020-05-15
- Refactor and expose more metadata (breaking!)
- Skip unplayable tracks instead of stopping 
- Fetch context for Spotify Collection types as well
- Autoplay - `uid` is optional for playlist stations 

## [0.2.0] - 2020-04-17
- Use a toml based configuration file 
- Enabled gapless playback as default 
- Handle reconnects more gracefully 

## [0.1.9] - 2019-11-08
- Tweak Discovery with more logging
- Exit on Session errors

## [0.1.8] - 2019-11-06
- Add support for podcasts
- Autoplay similar songs when your music ends
 
## [0.1.7] - 2019-07-30
- Refine dropped session handling 
- Add a flag (`LIBRESPOT_RATE_RESAMPLE`) to allow resampling with ALSA
- Refactor Volume control, allow for a fixed volume option 

## [0.1.6] - 2019-04-29
- Fix high CPU usage 

## [0.1.5] - 2019-04-14
- Exit when server connection is closed and also warn the front end
- Better handling of dropped sessions (v2)

## [0.1.4] - 2019-03-26
- Added support for Dailymixes
- Faster track changes
- Multiple small fixes

## [0.1.3] - 2019-02-25
- Better handling of dropped sessions
- Fix for track changing issues (breaking changes in Spotify API)

## [0.1.2] - 2018-11-21
- Fix multiple sockets

## [0.1.1] - 2018-10-16
- Bump RustCrypto crates to fix UB on aarch64
- Implement support for dynamic playlists (Radio)

## [0.1.0] - 2018-10-09
- Initial release
