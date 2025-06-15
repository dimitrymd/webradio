// static/js/optimized-radio-player.js - Fixed Audio Source Handling

// Radio state management
const radioState = {
    audioElement: null,
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    currentTrack: null,
    serverPosition: 0,
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent),
    
    // Timers
    nowPlayingTimer: null,
    heartbeatTimer: null,
    
    // Connection
    connectionId: null,
    consecutiveErrors: 0,
    isReconnecting: false,
    lastSuccessfulConnection: null,
    
    // UI state
    isTogglingConnection: false
};

// UI Elements
const elements = {
    startBtn: null,
    muteBtn: null,
    volumeControl: null,
    statusMessage: null,
    listenerCount: null,
    currentTitle: null,
    currentArtist: null,
    currentAlbum: null,
    currentPosition: null,
    currentDuration: null,
    progressBar: null
};

// Initialize the radio player
function initRadioPlayer() {
    console.log('🎵 ChillOut Radio - True Radio Mode v3.2 (Optimized)...');
    console.log('📻 Server-controlled playback only - no track control');
    
    // Get UI elements
    elements.startBtn = document.getElementById('start-btn');
    elements.muteBtn = document.getElementById('mute-btn');
    elements.volumeControl = document.getElementById('volume');
    elements.statusMessage = document.getElementById('status-message');
    elements.listenerCount = document.getElementById('listener-count');
    elements.currentTitle = document.getElementById('current-title');
    elements.currentArtist = document.getElementById('current-artist');
    elements.currentAlbum = document.getElementById('current-album');
    elements.currentPosition = document.getElementById('current-position');
    elements.currentDuration = document.getElementById('current-duration');
    elements.progressBar = document.getElementById('progress-bar');
    
    // Check required elements
    if (!elements.startBtn || !elements.muteBtn || !elements.volumeControl) {
        console.error('❌ Required UI elements not found');
        showStatus('❌ Player initialization failed - missing UI elements', true);
        return;
    }
    
    // Remove any skip/next/previous buttons if they exist
    const skipButtons = document.querySelectorAll('[id*="skip"], [id*="next"], [id*="prev"], [id*="switch"]');
    skipButtons.forEach(btn => {
        btn.style.display = 'none';
        btn.disabled = true;
    });
    
    // Set up event listeners
    setupEventListeners();
    
    // Load saved settings
    loadSettings();
    
    // Start fetching track info
    startTrackInfoUpdates();
    
    // Initial health check
    performHealthCheck();
    
    console.log('✅ Radio player initialized in true radio mode');
    showStatus('📻 True Radio Mode - Click "Tune In" to listen');
}

// Set up event listeners
function setupEventListeners() {
    // Main toggle button
    elements.startBtn.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        
        if (radioState.isTogglingConnection) {
            console.log('⏸️ Ignoring click - already toggling connection');
            return;
        }
        
        toggleRadio();
    });
    
    elements.muteBtn.addEventListener('click', toggleMute);
    elements.volumeControl.addEventListener('input', (e) => {
        updateVolume(parseFloat(e.target.value));
    });
    
    // Handle page visibility changes
    document.addEventListener('visibilitychange', () => {
        if (document.hidden && radioState.isPlaying) {
            console.log('📱 Page hidden - maintaining radio connection');
        } else if (!document.hidden && radioState.isPlaying) {
            console.log('📱 Page visible - checking radio connection');
            checkConnectionHealth();
        }
    });
}

// Perform initial health check
async function performHealthCheck() {
    try {
        console.log('🔍 Performing server health check...');
        const response = await fetch('/api/health', {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (response.ok) {
            const data = await response.json();
            console.log('✅ Server health check passed:', data);
            showStatus(`📻 Server ready - ${data.active_listeners || 0} listeners online`);
        } else {
            console.warn('⚠️ Server health check failed:', response.status);
            showStatus('⚠️ Server connection issues detected', true);
        }
    } catch (error) {
        console.error('❌ Health check failed:', error);
        showStatus('❌ Cannot connect to radio server', true);
    }
}

// Toggle radio on/off
async function toggleRadio() {
    console.log('🔄 Toggle radio clicked, current state:', {
        isPlaying: radioState.isPlaying,
        isReconnecting: radioState.isReconnecting,
        isTogglingConnection: radioState.isTogglingConnection
    });
    
    radioState.isTogglingConnection = true;
    
    try {
        if (radioState.isPlaying) {
            await stopRadio();
        } else {
            await startRadio();
        }
    } catch (error) {
        console.error('❌ Error during toggle:', error);
        showStatus(`❌ Toggle failed: ${error.message}`, true);
    } finally {
        setTimeout(() => {
            radioState.isTogglingConnection = false;
        }, 500);
    }
}

// Start radio
async function startRadio() {
    console.log('🎵 Starting radio...');
    
    if (radioState.isPlaying || radioState.isReconnecting) {
        console.log('Already playing or reconnecting');
        return;
    }
    
    radioState.isPlaying = true;
    radioState.isReconnecting = false;
    radioState.consecutiveErrors = 0;
    
    updateUIForConnecting();
    
    try {
        await createAudioElement();
        await startStreaming();
        updateUIForConnected();
        
    } catch (error) {
        console.error('❌ Failed to start radio:', error);
        radioState.isPlaying = false;
        updateUIForDisconnected();
        showStatus(`❌ Failed to start: ${error.message}`, true);
        throw error;
    }
}

// Update UI functions
function updateUIForConnecting() {
    elements.startBtn.disabled = true;
    elements.startBtn.textContent = '📻 Connecting...';
    elements.startBtn.dataset.connected = 'connecting';
    showStatus('📻 Connecting to live radio stream...');
}

function updateUIForConnected() {
    elements.startBtn.disabled = false;
    elements.startBtn.textContent = '📻 Disconnect';
    elements.startBtn.dataset.connected = 'true';
    showStatus('📻 🎵 Live on ChillOut Radio!');
}

function updateUIForDisconnected() {
    elements.startBtn.disabled = false;
    elements.startBtn.textContent = '📻 Tune In';
    elements.startBtn.dataset.connected = 'false';
}

// Create audio element
async function createAudioElement() {
    console.log('🔊 Creating audio element...');
    
    // Clean up existing element properly
    if (radioState.audioElement) {
        try {
            radioState.audioElement.pause();
            // Remove event listeners before clearing src
            radioState.audioElement.removeEventListener('error', handleAudioError);
            radioState.audioElement.removeEventListener('loadstart', handleLoadStart);
            radioState.audioElement.removeEventListener('canplay', handleCanPlay);
            radioState.audioElement.removeEventListener('playing', handlePlaying);
            radioState.audioElement.removeEventListener('waiting', handleWaiting);
            radioState.audioElement.removeEventListener('ended', handleEnded);
            
            // Clear source properly
            radioState.audioElement.removeAttribute('src');
            radioState.audioElement.load();
        } catch (error) {
            console.warn('⚠️ Error during audio cleanup:', error);
        }
        radioState.audioElement = null;
    }
    
    // Small delay to ensure cleanup
    await new Promise(resolve => setTimeout(resolve, 100));
    
    try {
        radioState.audioElement = new Audio();
        
        if (!radioState.audioElement) {
            throw new Error('Failed to create Audio element');
        }
        
        // Set properties
        radioState.audioElement.volume = radioState.volume;
        radioState.audioElement.muted = radioState.isMuted;
        radioState.audioElement.crossOrigin = "anonymous";
        radioState.audioElement.preload = 'none'; // Don't preload
        
        // Set buffering to minimum for faster start
        if ('mozPreservesPitch' in radioState.audioElement) {
            radioState.audioElement.mozPreservesPitch = false;
        }
        
        if (radioState.isIOS) {
            radioState.audioElement.playsInline = true;
        }
        
        console.log('✅ Audio element created successfully');
        
    } catch (error) {
        console.error('❌ Failed to create audio element:', error);
        radioState.audioElement = null;
        throw new Error(`Audio creation failed: ${error.message}`);
    }
}

// Audio event handlers
function handleLoadStart() {
    console.log('🔄 Audio: Load started');
    showStatus('📻 Connecting to stream...');
}

function handleCanPlay() {
    console.log('✅ Audio: Can play');
    showStatus('📻 Stream ready...');
}

function handlePlaying() {
    console.log('▶️ Audio: Playing');
    showStatus('📻 🎵 Live on ChillOut Radio!');
    radioState.consecutiveErrors = 0;
    radioState.lastSuccessfulConnection = Date.now();
}

function handleWaiting() {
    console.log('⏳ Audio: Buffering');
    showStatus('📻 Buffering...');
}

function handleEnded() {
    console.log('🔄 Stream ended - this should not happen in radio mode');
    if (radioState.isPlaying) {
        scheduleReconnect('Stream ended unexpectedly');
    }
}

// Handle audio errors
function handleAudioError(e) {
    const error = e.target.error;
    let errorMsg = 'Unknown error';
    let shouldReconnect = true;
    
    if (error) {
        switch (error.code) {
            case MediaError.MEDIA_ERR_ABORTED:
                errorMsg = 'Playback aborted';
                shouldReconnect = false;
                break;
            case MediaError.MEDIA_ERR_NETWORK:
                errorMsg = 'Network error';
                break;
            case MediaError.MEDIA_ERR_DECODE:
                errorMsg = 'Audio decoding error';
                break;
            case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
                errorMsg = 'Audio format not supported';
                shouldReconnect = false;
                break;
            default:
                errorMsg = `Media error (code ${error.code})`;
        }
    }
    
    console.error('❌ Audio error:', errorMsg, error);
    radioState.consecutiveErrors++;
    
    if (!radioState.isPlaying) {
        console.log('⏭️ Ignoring audio error - player was stopped');
        return;
    }
    
    if (shouldReconnect && radioState.consecutiveErrors < 5) {
        scheduleReconnect(errorMsg);
    } else {
        stopRadio(true);
    }
}

// Start streaming with proper source setting
async function startStreaming() {
    if (!radioState.audioElement) {
        throw new Error('Audio element is null');
    }
    
    console.log('🌐 Starting streaming...');
    
    // Check server status first
    try {
        const statusResponse = await fetch('/stream-status', {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (!statusResponse.ok) {
            throw new Error(`Server returned ${statusResponse.status}`);
        }
        
        const statusData = await statusResponse.json();
        if (!statusData.streaming) {
            throw new Error('Server is not currently streaming');
        }
        
        console.log('✅ Server streaming status confirmed:', statusData);
    } catch (error) {
        console.error('❌ Server status check failed:', error);
        throw new Error(`Server not ready: ${error.message}`);
    }
    
    // Build stream URL
    const timestamp = Date.now();
    let streamUrl = `/direct-stream?t=${timestamp}`;
    
    if (radioState.isIOS) {
        streamUrl += '&platform=ios';
    } else if (radioState.isMobile) {
        streamUrl += '&platform=mobile';
    } else {
        streamUrl += '&platform=desktop';
    }
    
    console.log('🌐 Stream URL:', streamUrl);
    
    // Set up event listeners BEFORE setting src
    radioState.audioElement.addEventListener('loadstart', handleLoadStart);
    radioState.audioElement.addEventListener('canplay', handleCanPlay);
    radioState.audioElement.addEventListener('playing', handlePlaying);
    radioState.audioElement.addEventListener('waiting', handleWaiting);
    radioState.audioElement.addEventListener('ended', handleEnded);
    radioState.audioElement.addEventListener('error', handleAudioError);
    
    // Add additional debug listeners
    radioState.audioElement.addEventListener('loadedmetadata', () => {
        console.log('✅ Audio: Metadata loaded');
    });
    
    radioState.audioElement.addEventListener('loadeddata', () => {
        console.log('✅ Audio: Data loaded');
    });
    
    radioState.audioElement.addEventListener('canplaythrough', () => {
        console.log('✅ Audio: Can play through');
    });
    
    radioState.audioElement.addEventListener('stalled', () => {
        console.log('⚠️ Audio: Stalled');
        showStatus('📻 Stream stalled - retrying...', true);
    });
    
    radioState.audioElement.addEventListener('suspend', () => {
        console.log('⚠️ Audio: Suspended');
    });
    
    radioState.audioElement.addEventListener('progress', () => {
        const buffered = radioState.audioElement.buffered;
        if (buffered.length > 0) {
            const bufferedEnd = buffered.end(buffered.length - 1);
            const duration = radioState.audioElement.duration;
            console.log(`📊 Buffered: ${bufferedEnd.toFixed(1)}s${duration ? ` of ${duration.toFixed(1)}s` : ''}`);
        }
    });
    
    // Set source and start playing
    return new Promise((resolve, reject) => {
        let playAttempted = false;
        let timeoutId;
        
        // First, test if the stream endpoint is working
        console.log('🔍 Testing stream endpoint...');
        fetch(streamUrl, { method: 'HEAD' })
            .then(response => {
                console.log('📡 Stream endpoint response:', response.status, response.headers.get('content-type'));
                if (!response.ok) {
                    throw new Error(`Stream endpoint returned ${response.status}`);
                }
            })
            .catch(error => {
                console.error('❌ Stream endpoint test failed:', error);
            });
        
        const cleanup = () => {
            if (timeoutId) clearTimeout(timeoutId);
        };
        
        const handleSuccess = () => {
            cleanup();
            console.log('✅ Streaming started successfully');
            resolve();
        };
        
        const handleError = (error) => {
            cleanup();
            console.error('❌ Streaming failed:', error);
            reject(error);
        };
        
        // Set timeout
        timeoutId = setTimeout(() => {
            if (!playAttempted) {
                handleError(new Error('Connection timeout after 15 seconds'));
            }
        }, 15000);
        
        // Verify audio element still exists
        if (!radioState.audioElement) {
            handleError(new Error('Audio element became null'));
            return;
        }
        
        // Set up one-time playing listener
        const playingHandler = () => {
            radioState.audioElement.removeEventListener('playing', playingHandler);
            handleSuccess();
        };
        
        radioState.audioElement.addEventListener('playing', playingHandler);
        
        try {
            // Set the source
            console.log('🎵 Setting audio source to:', streamUrl);
            radioState.audioElement.src = streamUrl;
            
            // Log element state
            console.log('📻 Audio element state:', {
                src: radioState.audioElement.src,
                readyState: radioState.audioElement.readyState,
                networkState: radioState.audioElement.networkState,
                paused: radioState.audioElement.paused
            });
            
            // Attempt to play
            playAttempted = true;
            console.log('▶️ Calling play()...');
            const playPromise = radioState.audioElement.play();
            
            if (playPromise !== undefined) {
                playPromise.then(() => {
                    console.log('✅ Play promise resolved');
                }).catch(error => {
                    console.error('❌ Play promise rejected:', error);
                    radioState.audioElement.removeEventListener('playing', playingHandler);
                    
                    if (error.name === 'NotAllowedError') {
                        // Autoplay blocked
                        cleanup();
                        showStatus('🔊 Click to enable audio playback', true);
                        elements.startBtn.textContent = '🔊 Enable Audio';
                        elements.startBtn.disabled = false;
                        
                        // Set up one-time click handler
                        const enableAudio = async () => {
                            elements.startBtn.removeEventListener('click', enableAudio);
                            try {
                                if (radioState.audioElement) {
                                    await radioState.audioElement.play();
                                    setupEventListeners(); // Restore normal handlers
                                    updateUIForConnected();
                                    showStatus('📻 🎵 Live on ChillOut Radio!');
                                    resolve();
                                } else {
                                    reject(new Error('Audio element is null'));
                                }
                            } catch (playError) {
                                console.error('❌ Manual play failed:', playError);
                                reject(playError);
                            }
                        };
                        
                        elements.startBtn.addEventListener('click', enableAudio);
                    } else {
                        handleError(error);
                    }
                });
            }
        } catch (error) {
            handleError(error);
        }
    });
}

// Stop radio
async function stopRadio(isError = false) {
    console.log('⏹️ Stopping radio...');
    
    radioState.isPlaying = false;
    radioState.isReconnecting = false;
    
    if (radioState.audioElement) {
        try {
            radioState.audioElement.pause();
            
            // Remove all event listeners
            radioState.audioElement.removeEventListener('error', handleAudioError);
            radioState.audioElement.removeEventListener('loadstart', handleLoadStart);
            radioState.audioElement.removeEventListener('canplay', handleCanPlay);
            radioState.audioElement.removeEventListener('playing', handlePlaying);
            radioState.audioElement.removeEventListener('waiting', handleWaiting);
            radioState.audioElement.removeEventListener('ended', handleEnded);
            
            // Clear source
            radioState.audioElement.removeAttribute('src');
            radioState.audioElement.load();
        } catch (error) {
            console.warn('⚠️ Error during audio cleanup:', error);
        }
        radioState.audioElement = null;
    }
    
    clearTimers();
    radioState.connectionId = null;
    radioState.consecutiveErrors = 0;
    
    updateUIForDisconnected();
    
    if (!isError) {
        showStatus('📻 Disconnected from radio');
        console.log('✅ Radio stopped cleanly');
    } else {
        console.log('⚠️ Radio stopped due to error');
    }
}

// Schedule reconnect
function scheduleReconnect(reason = 'Unknown error') {
    if (radioState.isReconnecting || !radioState.isPlaying) {
        console.log('⏭️ Skipping reconnect - already reconnecting or not playing');
        return;
    }
    
    radioState.isReconnecting = true;
    console.log(`🔄 Scheduling reconnect due to: ${reason}`);
    
    if (radioState.audioElement) {
        try {
            radioState.audioElement.pause();
            radioState.audioElement.removeAttribute('src');
            radioState.audioElement.load();
            radioState.audioElement = null;
        } catch (error) {
            console.warn('⚠️ Error during reconnect cleanup:', error);
        }
    }
    
    const delay = Math.min(1000 * Math.pow(2, radioState.consecutiveErrors), 10000);
    
    showStatus(`📻 Reconnecting in ${Math.round(delay/1000)}s... (${reason})`);
    
    setTimeout(async () => {
        if (radioState.isPlaying && radioState.isReconnecting) {
            console.log(`🔄 Attempting reconnect (attempt ${radioState.consecutiveErrors + 1})`);
            
            try {
                radioState.isReconnecting = false;
                await createAudioElement();
                
                if (!radioState.audioElement) {
                    throw new Error('Failed to create audio element during reconnect');
                }
                
                await startStreaming();
                showStatus('📻 🎵 Reconnected successfully!');
                radioState.consecutiveErrors = 0;
                
            } catch (error) {
                console.error('❌ Reconnect failed:', error);
                radioState.consecutiveErrors++;
                
                if (radioState.consecutiveErrors >= 5) {
                    console.error('❌ Max reconnection attempts reached');
                    showStatus('❌ Connection failed - please try again later', true);
                    await stopRadio(true);
                } else {
                    scheduleReconnect('Reconnection failed');
                }
            }
        } else {
            console.log('⏭️ Reconnect cancelled - player stopped');
            radioState.isReconnecting = false;
        }
    }, delay);
}

// Check connection health
async function checkConnectionHealth() {
    if (!radioState.isPlaying) return;
    
    try {
        const response = await fetch('/api/heartbeat', {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (!response.ok) {
            console.warn('⚠️ Heartbeat failed:', response.status);
            return;
        }
        
        const data = await response.json();
        
        if (data.active_listeners !== undefined && elements.listenerCount) {
            elements.listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${data.active_listeners}`;
        }
        
        console.log('💓 Connection healthy, listeners:', data.active_listeners);
        
    } catch (error) {
        console.error('❌ Health check failed:', error);
    }
}

// Toggle mute
function toggleMute() {
    radioState.isMuted = !radioState.isMuted;
    
    if (radioState.audioElement) {
        radioState.audioElement.muted = radioState.isMuted;
    }
    
    elements.muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
    
    try {
        localStorage.setItem('radioMuted', radioState.isMuted.toString());
    } catch (e) {
        console.log('Could not save mute setting');
    }
}

// Update volume
function updateVolume(volume) {
    radioState.volume = volume;
    
    if (radioState.audioElement) {
        radioState.audioElement.volume = volume;
    }
    
    try {
        localStorage.setItem('radioVolume', volume.toString());
    } catch (e) {
        console.log('Could not save volume setting');
    }
}

// Load settings
function loadSettings() {
    try {
        const savedVolume = localStorage.getItem('radioVolume');
        if (savedVolume !== null) {
            const volume = parseFloat(savedVolume);
            if (!isNaN(volume) && volume >= 0 && volume <= 1) {
                elements.volumeControl.value = volume;
                radioState.volume = volume;
            }
        }
        
        const savedMuted = localStorage.getItem('radioMuted');
        if (savedMuted !== null) {
            radioState.isMuted = savedMuted === 'true';
            elements.muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
        }
    } catch (e) {
        console.log('Could not load settings from localStorage');
    }
}

// Start track info updates
function startTrackInfoUpdates() {
    clearTimers();
    
    radioState.nowPlayingTimer = setInterval(fetchNowPlaying, 5000);
    radioState.heartbeatTimer = setInterval(sendHeartbeat, 15000);
    
    fetchNowPlaying();
}

// Clear timers
function clearTimers() {
    if (radioState.nowPlayingTimer) {
        clearInterval(radioState.nowPlayingTimer);
        radioState.nowPlayingTimer = null;
    }
    if (radioState.heartbeatTimer) {
        clearInterval(radioState.heartbeatTimer);
        radioState.heartbeatTimer = null;
    }
}

// Fetch now playing
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing', {
            headers: {
                'Cache-Control': 'no-cache',
                'Accept': 'application/json'
            }
        });
        
        if (!response.ok) {
            console.error('❌ Now playing API error:', response.status);
            return;
        }
        
        const data = await response.json();
        updateTrackInfo(data);
        
    } catch (error) {
        console.error('❌ Error fetching now playing:', error);
    }
}

// Update track info
function updateTrackInfo(info) {
    try {
        if (!info || typeof info !== 'object') {
            console.warn('⚠️ Invalid track info received:', info);
            return;
        }
        
        if (info.error) {
            console.error('❌ Server error in track info:', info.error);
            return;
        }
        
        radioState.currentTrack = info;
        radioState.serverPosition = info.radio_position || info.playback_position || 0;
        
        // Update track display
        if (elements.currentTitle && info.title) {
            elements.currentTitle.textContent = info.title;
        }
        if (elements.currentArtist && info.artist) {
            elements.currentArtist.textContent = info.artist;
        }
        if (elements.currentAlbum && info.album) {
            elements.currentAlbum.textContent = info.album;
        }
        
        // Update duration
        if (elements.currentDuration && info.duration && info.duration > 0) {
            elements.currentDuration.textContent = formatTime(info.duration);
        }
        
        // Update position
        if (elements.currentPosition) {
            elements.currentPosition.textContent = formatTime(radioState.serverPosition);
        }
        
        // Update progress bar
        if (elements.progressBar && info.duration && info.duration > 0) {
            const percent = (radioState.serverPosition / info.duration) * 100;
            elements.progressBar.style.width = `${Math.min(100, Math.max(0, percent))}%`;
        }
        
        // Update listener count
        if (elements.listenerCount && typeof info.active_listeners === 'number') {
            elements.listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${info.active_listeners}`;
        }
        
        // Update document title
        if (info.title && info.artist) {
            document.title = `📻 ${info.title} - ${info.artist} | ChillOut Radio`;
        }
        
    } catch (error) {
        console.error('❌ Error updating track info:', error);
    }
}

// Send heartbeat
async function sendHeartbeat() {
    if (!radioState.connectionId) {
        return;
    }
    
    try {
        const response = await fetch(`/api/heartbeat?connection_id=${radioState.connectionId}`, {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (response.ok) {
            const data = await response.json();
            
            if (data.active_listeners !== undefined && elements.listenerCount) {
                elements.listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${data.active_listeners}`;
            }
        }
    } catch (error) {
        console.log('💓 Heartbeat failed (this is OK):', error.message);
    }
}

// Show status message
function showStatus(message, isError = false, autoHide = true) {
    console.log(`Status: ${message}`);
    
    if (elements.statusMessage) {
        elements.statusMessage.textContent = message;
        elements.statusMessage.style.display = 'block';
        elements.statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
        elements.statusMessage.style.backgroundColor = isError ? '#fdf2f2' : '#f0f8ff';
        
        if (!isError && autoHide) {
            setTimeout(() => {
                if (elements.statusMessage.textContent === message) {
                    elements.statusMessage.style.display = 'none';
                }
            }, 4000);
        }
    }
}

// Format time helper
function formatTime(seconds) {
    if (!seconds || seconds < 0 || !isFinite(seconds)) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Initialize when page loads
document.addEventListener('DOMContentLoaded', () => {
    try {
        initRadioPlayer();
        console.log('🎵 ChillOut Radio v3.2 - True Radio Mode (Optimized)');
        console.log('📻 Server-controlled playback only');
    } catch (error) {
        console.error('❌ Failed to initialize radio player:', error);
        alert('Radio player failed to initialize. Please refresh the page.');
    }
});

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    console.log('📱 Page unloading - cleaning up');
    clearTimers();
    if (radioState.audioElement) {
        radioState.audioElement.pause();
        radioState.audioElement.removeAttribute('src');
    }
});

// Handle online/offline events
window.addEventListener('online', () => {
    console.log('🌐 Network: Online');
    if (radioState.isPlaying && !radioState.audioElement) {
        showStatus('📻 Network restored - reconnecting...', false);
        scheduleReconnect('Network restored');
    }
});

window.addEventListener('offline', () => {
    console.log('📡 Network: Offline');
    showStatus('📡 Network offline - will reconnect when available', true);
});

// Debug object
window.ChillOutRadio = {
    state: radioState,
    elements: elements,
    start: startRadio,
    stop: stopRadio,
    fetchInfo: fetchNowPlaying,
    healthCheck: performHealthCheck,
    version: '3.2.0-optimized',
    mode: 'server-controlled',
    
    getAudioState: () => {
        if (!radioState.audioElement) return 'No audio element';
        return {
            paused: radioState.audioElement.paused,
            ended: radioState.audioElement.ended,
            error: radioState.audioElement.error,
            readyState: radioState.audioElement.readyState,
            networkState: radioState.audioElement.networkState,
            src: radioState.audioElement.src,
            currentSrc: radioState.audioElement.currentSrc,
            buffered: radioState.audioElement.buffered.length > 0 ? {
                start: radioState.audioElement.buffered.start(0),
                end: radioState.audioElement.buffered.end(radioState.audioElement.buffered.length - 1)
            } : null
        };
    },
    
    testDirectStream: async () => {
        try {
            const url = `/direct-stream?t=${Date.now()}`;
            const response = await fetch(url, { method: 'HEAD' });
            console.log('Direct stream test:', {
                url: url,
                status: response.status,
                contentType: response.headers.get('content-type'),
                headers: Object.fromEntries(response.headers.entries())
            });
            return response.ok;
        } catch (error) {
            console.error('Direct stream test failed:', error);
            return false;
        }
    },
    
    getBufferStatus: () => {
        if (!radioState.audioElement || radioState.audioElement.buffered.length === 0) {
            return 'No buffer';
        }
        const buffered = radioState.audioElement.buffered;
        const currentTime = radioState.audioElement.currentTime;
        let bufferedAhead = 0;
        
        for (let i = 0; i < buffered.length; i++) {
            if (buffered.start(i) <= currentTime && currentTime <= buffered.end(i)) {
                bufferedAhead = buffered.end(i) - currentTime;
                break;
            }
        }
        
        return {
            bufferedAhead: bufferedAhead.toFixed(2) + 's',
            totalBuffered: (buffered.end(buffered.length - 1) - buffered.start(0)).toFixed(2) + 's',
            bufferRanges: Array.from({length: buffered.length}, (_, i) => ({
                start: buffered.start(i).toFixed(2),
                end: buffered.end(i).toFixed(2)
            }))
        };
    }
};

console.log('🎵 ChillOut Radio v3.2 - Optimized for fast connection');
console.log('📻 Instant playback with minimal buffering');
console.log('🔧 Debug: window.ChillOutRadio available');