# Vollibrespot
Spotify Connect for [Volumio2](https://github.com/volumio/volumio2).

This is a Spotify connect implementation using [`librespot`](https://github.com/librespot-org/librespot). This daemon is customised to work as the backend for the [`volspotconnect2`](https://github.com/balbuze/volumio-plugins/tree/master/plugins/music_service/volspotconnect2) Spotify Connect plugin by:

* Providing metadata in `json`
* Keeping track of state changes (`Play`|`Pause`|`Seek`)
* etc,

# Usage
Installing `volspotconnect2` via the Volumio Plugin Manager should automatically install `vollibrespot`


##### Disclaimer
Using this code to connect to Spotify's API is probably forbidden by them.
Use at your own risk.

Note: `vollibrespot` only works with Spotify Premium
