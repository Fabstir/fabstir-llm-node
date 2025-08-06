# Deployment Configurations

## Production
- `docker-compose.production.yml` - Main production deployment

## Phase 4.3.1 (Historical)
Archive of docker-compose files used during Phase 4.3.1 development:
- `docker-compose.phase-4.3.1-final.yml` - Final working configuration
- Other files kept for reference

## Usage
```bash
# Production deployment
docker-compose -f deployment/docker-compose.production.yml up -d

# Stop services
docker-compose -f deployment/docker-compose.production.yml down
```
