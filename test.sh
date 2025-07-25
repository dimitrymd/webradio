#!/bin/bash

echo "Testing WebRadio endpoints..."

# Check health
echo -e "\n=== Health Check ==="
curl -s http://localhost:8000/api/health | jq .

# Check debug info
echo -e "\n=== Debug Info ==="
curl -s http://localhost:8000/api/debug | jq .

# Check playlist
echo -e "\n=== Playlist ==="
curl -s http://localhost:8000/api/playlist | jq '.tracks | length'

# Test audio stream
echo -e "\n=== Testing Audio Stream ==="
curl -I http://localhost:8000/stream

# Download a few seconds of audio
echo -e "\n=== Downloading 3 seconds of audio ==="
timeout 3 curl -s http://localhost:8000/stream -o test.mp3
ls -la test.mp3 2>/dev/null || echo "No audio data received"