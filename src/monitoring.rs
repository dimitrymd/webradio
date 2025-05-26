// src/monitoring.rs - Performance monitoring for True Radio

use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub active_listeners: usize,
    pub bytes_broadcast: u64,
    pub chunks_sent: u64,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub disk_read_rate: u64, // bytes/sec
    pub network_send_rate: u64, // bytes/sec
    pub broadcast_lag: Duration,
    pub timestamp: Instant,
}

pub struct PerformanceMonitor {
    metrics: Arc<RwLock<VecDeque<PerformanceMetrics>>>,
    start_time: Instant,
    last_bytes: u64,
    last_chunks: u64,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(VecDeque::with_capacity(3600))), // 1 hour of data
            start_time: Instant::now(),
            last_bytes: 0,
            last_chunks: 0,
        }
    }
    
    pub fn record_metrics(
        &mut self,
        active_listeners: usize,
        total_bytes: u64,
        total_chunks: u64,
        broadcast_lag: Duration,
    ) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.start_time).as_secs_f32();
        
        // Calculate rates
        let bytes_diff = total_bytes - self.last_bytes;
        let chunks_diff = total_chunks - self.last_chunks;
        
        self.last_bytes = total_bytes;
        self.last_chunks = total_chunks;
        
        // Get system metrics
        let memory_usage = self.get_memory_usage();
        let cpu_usage = self.get_cpu_usage();
        
        let metrics = PerformanceMetrics {
            active_listeners,
            bytes_broadcast: total_bytes,
            chunks_sent: total_chunks,
            cpu_usage,
            memory_usage,
            disk_read_rate: bytes_diff, // per second
            network_send_rate: bytes_diff * active_listeners as u64, // total network
            broadcast_lag,
            timestamp: now,
        };
        
        let mut history = self.metrics.write();
        if history.len() >= 3600 {
            history.pop_front();
        }
        history.push_back(metrics);
    }
    
    fn get_memory_usage(&self) -> u64 {
        // Linux-specific memory usage
        #[cfg(target_os = "linux")]
        {
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        if let Some(kb_str) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = kb_str.parse::<u64>() {
                                return kb * 1024; // Convert to bytes
                            }
                        }
                    }
                }
            }
        }
        0
    }
    
    fn get_cpu_usage(&self) -> f32 {
        // Simplified CPU usage - would need more sophisticated implementation
        0.0
    }
    
    pub fn get_current_metrics(&self) -> Option<PerformanceMetrics> {
        self.metrics.read().back().cloned()
    }
    
    pub fn get_statistics(&self) -> serde_json::Value {
        let history = self.metrics.read();
        
        if history.is_empty() {
            return serde_json::json!({
                "error": "No metrics available"
            });
        }
        
        let latest = history.back().unwrap();
        let uptime = Instant::now().duration_since(self.start_time);
        
        // Calculate averages
        let avg_listeners = history.iter().map(|m| m.active_listeners).sum::<usize>() / history.len();
        let max_listeners = history.iter().map(|m| m.active_listeners).max().unwrap_or(0);
        
        serde_json::json!({
            "uptime_seconds": uptime.as_secs(),
            "current": {
                "active_listeners": latest.active_listeners,
                "memory_usage_mb": latest.memory_usage / (1024 * 1024),
                "cpu_usage_percent": latest.cpu_usage,
                "disk_read_kbps": (latest.disk_read_rate * 8) / 1024,
                "network_send_mbps": (latest.network_send_rate * 8) / (1024 * 1024),
                "broadcast_lag_ms": latest.broadcast_lag.as_millis(),
            },
            "totals": {
                "bytes_broadcast": latest.bytes_broadcast,
                "chunks_sent": latest.chunks_sent,
                "gb_broadcast": latest.bytes_broadcast as f64 / (1024.0 * 1024.0 * 1024.0),
            },
            "statistics": {
                "average_listeners": avg_listeners,
                "max_listeners": max_listeners,
                "efficiency": {
                    "bytes_per_listener": if latest.active_listeners > 0 {
                        latest.bytes_broadcast / latest.active_listeners as u64
                    } else { 0 },
                    "single_reader": true,
                    "broadcast_model": "true_radio",
                }
            }
        })
    }
}

// Add monitoring endpoint to handlers.rs
#[rocket::get("/api/metrics")]
pub async fn get_metrics(
    monitor: &State<Arc<RwLock<PerformanceMonitor>>>
) -> rocket::serde::json::Json<serde_json::Value> {
    let monitor = monitor.read();
    rocket::serde::json::Json(monitor.get_statistics())
}