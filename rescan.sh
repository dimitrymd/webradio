#!/bin/bash

echo "Forcing playlist rescan..."

# Remove existing playlist to force rescan
if [ -f "music/playlist.json" ]; then
    echo "Removing existing playlist.json"
    rm music/playlist.json
fi

# Just start the server briefly to rescan
echo "Starting server to rescan music files..."
timeout 5 cargo run 2>&1 | grep -E "(Scanning|Found|Track:|Bitrate:)"

echo -e "\nPlaylist rescanned. Check music/playlist.json for results."

# Show the playlist with bitrates
if [ -f "music/playlist.json" ]; then
    echo -e "\nTracks found:"
    cat music/playlist.json | jq -r '.tracks[] | "\(.artist) - \(.title) [\(.bitrate/1000)kbps]"'
fi