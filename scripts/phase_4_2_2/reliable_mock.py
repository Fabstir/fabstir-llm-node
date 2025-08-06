#!/usr/bin/env python3
from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import time

class ReliableMock(BaseHTTPRequestHandler):
    count = 0
    start_time = time.time()
    
    def do_GET(self):
        if 'health' in self.path:
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            response = {
                "status": "healthy",
                "requests": self.count,
                "uptime": int(time.time() - self.start_time)
            }
            self.wfile.write(json.dumps(response).encode())
        else:
            self.send_response(404)
            self.end_headers()
    
    def do_POST(self):
        if 'vector' in self.path:
            ReliableMock.count += 1
            
            # Read the data
            length = int(self.headers.get('Content-Length', 0))
            if length:
                data = json.loads(self.rfile.read(length))
                vector_id = data.get('id', f'vec_{self.count}')
            else:
                vector_id = f'vec_{self.count}'
            
            # Send response
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            response = {"id": vector_id, "status": "inserted"}
            self.wfile.write(json.dumps(response).encode())
            
            # Progress indicator
            if self.count % 25 == 0:
                elapsed = time.time() - self.start_time
                rate = self.count / elapsed if elapsed > 0 else 0
                print(f"Processed {self.count} vectors ({rate:.1f}/sec)")
        else:
            self.send_response(404)
            self.end_headers()
    
    def log_message(self, *args):
        pass  # Suppress request logs

if __name__ == '__main__':
    print("=" * 50)
    print("RELIABLE Mock Vector DB")
    print("=" * 50)
    print("Port: 8080")
    print("Endpoints:")
    print("  GET  /api/v1/health")
    print("  POST /api/v1/vectors")
    print("=" * 50)
    print("Ready for testing!\n")
    
    try:
        server = HTTPServer(('', 8080), ReliableMock)
        server.serve_forever()
    except KeyboardInterrupt:
        print(f"\n\nShutdown: Processed {ReliableMock.count} total requests")
