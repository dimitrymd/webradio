// static/js/player/logger.js - Logging utility

export class Logger {
    constructor() {
        this.debugMode = true; // Can be configured
        this.logHistory = [];
        this.maxHistorySize = 100;
    }
    
    log(message, category = 'INFO', isError = false) {
        if (isError || this.debugMode) {
            const timestamp = new Date().toISOString().substr(11, 8);
            const logEntry = {
                timestamp,
                message,
                category,
                isError,
                time: Date.now()
            };
            
            // Add to history
            this.logHistory.unshift(logEntry);
            if (this.logHistory.length > this.maxHistorySize) {
                this.logHistory = this.logHistory.slice(0, this.maxHistorySize);
            }
            
            // Console output with styling
            const style = this.getLogStyle(category, isError);
            const logMessage = `[${timestamp}] [${category}] ${message}`;
            
            console[isError ? 'error' : 'log'](`%c${logMessage}`, style);
        }
    }
    
    error(message, category = 'ERROR') {
        this.log(message, category, true);
    }
    
    warn(message, category = 'WARN') {
        this.log(message, category, false);
        console.warn(`[${category}] ${message}`);
    }
    
    debug(message, category = 'DEBUG') {
        if (this.debugMode) {
            this.log(message, category, false);
        }
    }
    
    getLogStyle(category, isError) {
        if (isError) {
            return 'color: #e74c3c; font-weight: bold;';
        }
        
        const styles = {
            'MOBILE': 'color: #4CAF50; font-weight: bold;',
            'ANDROID': 'color: #FF9800; font-weight: bold;',
            'IOS': 'color: #ff6b6b; font-weight: bold;',
            'AUDIO': 'color: #2ecc71;',
            'CONTROL': 'color: #9b59b6;',
            'TRACK': 'color: #f39c12;',
            'API': 'color: #3498db;',
            'CONNECTION': 'color: #1abc9c;',
            'NETWORK': 'color: #34495e; font-weight: bold;',
            'HEARTBEAT': 'color: #e67e22;',
            'SYNC': 'color: #3498db; font-weight: bold;',
            'STORAGE': 'color: #95a5a6;',
            'WAKE': 'color: #f1c40f;',
            'BATTERY': 'color: #e74c3c; font-style: italic;',
            'PERFORMANCE': 'color: #8e44ad;',
            'CLEANUP': 'color: #7f8c8d;',
            'INIT': 'color: #2c3e50; font-weight: bold;',
            'UI': 'color: #16a085;',
            'DEBUG': 'color: #bdc3c7;',
            'DEFAULT': 'color: #2c3e50;'
        };
        
        return styles[category] || styles.DEFAULT;
    }
    
    // Get recent logs for debugging
    getRecentLogs(count = 20) {
        return this.logHistory.slice(0, count);
    }
    
    // Get logs by category
    getLogsByCategory(category, count = 10) {
        return this.logHistory
            .filter(entry => entry.category === category)
            .slice(0, count);
    }
    
    // Get error logs
    getErrors(count = 10) {
        return this.logHistory
            .filter(entry => entry.isError)
            .slice(0, count);
    }
    
    // Clear log history
    clearHistory() {
        this.logHistory = [];
        this.log('Log history cleared', 'LOGGER');
    }
    
    // Export logs for debugging
    exportLogs() {
        const logsData = {
            timestamp: new Date().toISOString(),
            userAgent: navigator.userAgent,
            url: window.location.href,
            logs: this.logHistory
        };
        
        return JSON.stringify(logsData, null, 2);
    }
    
    // Performance logging
    timeStart(label) {
        console.time(label);
    }
    
    timeEnd(label) {
        console.timeEnd(label);
    }
    
    // Group logging
    group(label) {
        console.group(label);
    }
    
    groupEnd() {
        console.groupEnd();
    }
    
    // Table logging for data
    table(data) {
        console.table(data);
    }
}

// Export singleton instance
export const logger = new Logger();