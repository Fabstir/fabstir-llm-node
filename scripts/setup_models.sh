#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Unified model setup script for new Fabstir LLM Node hosts
# Downloads all required models for vision, OCR, and embeddings

set -e  # Exit on error

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "${SCRIPT_DIR}")"

echo -e "${CYAN}======================================================${NC}"
echo -e "${CYAN}     Fabstir LLM Node - Model Setup Script            ${NC}"
echo -e "${CYAN}======================================================${NC}"
echo ""
echo "This script downloads all required models for running a"
echo "Fabstir LLM Node with full vision and RAG capabilities."
echo ""
echo -e "${BLUE}Models to be downloaded:${NC}"
echo "  1. Florence-2-large   - Image description (~2.6GB)"
echo "  2. PaddleOCR English  - Text extraction (~50MB)"
echo "  3. all-MiniLM-L6-v2   - Text embeddings (~90MB)"
echo ""
echo -e "${YELLOW}Total download size: ~2.7GB${NC}"
echo -e "${YELLOW}Total disk space needed: ~3GB${NC}"
echo ""
echo "Target directory: ${PROJECT_ROOT}/models/"
echo ""

# Parse arguments
SKIP_CONFIRM=false
SKIP_FLORENCE=false
SKIP_OCR=false
SKIP_EMBEDDINGS=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -y|--yes)
            SKIP_CONFIRM=true
            shift
            ;;
        --skip-florence)
            SKIP_FLORENCE=true
            shift
            ;;
        --skip-ocr)
            SKIP_OCR=true
            shift
            ;;
        --skip-embeddings)
            SKIP_EMBEDDINGS=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -y, --yes           Skip confirmation prompt"
            echo "  --skip-florence     Skip Florence-2 vision model"
            echo "  --skip-ocr          Skip PaddleOCR models"
            echo "  --skip-embeddings   Skip embedding model"
            echo "  -h, --help          Show this help message"
            echo ""
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

# Confirm with user
if [ "$SKIP_CONFIRM" = false ]; then
    read -p "Continue with download? [Y/n] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]] && [[ ! -z $REPLY ]]; then
        echo "Aborted."
        exit 0
    fi
fi

echo ""
echo -e "${GREEN}Starting model downloads...${NC}"
echo ""

# Track success/failure
FLORENCE_OK=false
OCR_OK=false
EMBEDDINGS_OK=false

# Download Florence-2 vision model
if [ "$SKIP_FLORENCE" = false ]; then
    echo -e "${CYAN}[1/3] Florence-2-large Vision Model${NC}"
    echo "----------------------------------------"
    if [ -x "${SCRIPT_DIR}/download_florence_model.sh" ]; then
        if "${SCRIPT_DIR}/download_florence_model.sh"; then
            FLORENCE_OK=true
            echo -e "${GREEN}Florence-2 setup complete!${NC}"
        else
            echo -e "${RED}Florence-2 setup failed!${NC}"
        fi
    else
        chmod +x "${SCRIPT_DIR}/download_florence_model.sh"
        if "${SCRIPT_DIR}/download_florence_model.sh"; then
            FLORENCE_OK=true
            echo -e "${GREEN}Florence-2 setup complete!${NC}"
        else
            echo -e "${RED}Florence-2 setup failed!${NC}"
        fi
    fi
    echo ""
else
    echo -e "${YELLOW}[1/3] Skipping Florence-2 (--skip-florence)${NC}"
    FLORENCE_OK=true
    echo ""
fi

# Download PaddleOCR models
if [ "$SKIP_OCR" = false ]; then
    echo -e "${CYAN}[2/3] PaddleOCR English Models${NC}"
    echo "----------------------------------------"
    if [ -x "${SCRIPT_DIR}/download_ocr_models.sh" ]; then
        if "${SCRIPT_DIR}/download_ocr_models.sh"; then
            OCR_OK=true
            echo -e "${GREEN}PaddleOCR setup complete!${NC}"
        else
            echo -e "${RED}PaddleOCR setup failed!${NC}"
        fi
    else
        chmod +x "${SCRIPT_DIR}/download_ocr_models.sh"
        if "${SCRIPT_DIR}/download_ocr_models.sh"; then
            OCR_OK=true
            echo -e "${GREEN}PaddleOCR setup complete!${NC}"
        else
            echo -e "${RED}PaddleOCR setup failed!${NC}"
        fi
    fi
    echo ""
else
    echo -e "${YELLOW}[2/3] Skipping PaddleOCR (--skip-ocr)${NC}"
    OCR_OK=true
    echo ""
fi

# Download embedding model
if [ "$SKIP_EMBEDDINGS" = false ]; then
    echo -e "${CYAN}[3/3] all-MiniLM-L6-v2 Embedding Model${NC}"
    echo "----------------------------------------"
    if [ -x "${SCRIPT_DIR}/download_embedding_model.sh" ]; then
        if "${SCRIPT_DIR}/download_embedding_model.sh"; then
            EMBEDDINGS_OK=true
            echo -e "${GREEN}Embedding model setup complete!${NC}"
        else
            echo -e "${RED}Embedding model setup failed!${NC}"
        fi
    else
        chmod +x "${SCRIPT_DIR}/download_embedding_model.sh"
        if "${SCRIPT_DIR}/download_embedding_model.sh"; then
            EMBEDDINGS_OK=true
            echo -e "${GREEN}Embedding model setup complete!${NC}"
        else
            echo -e "${RED}Embedding model setup failed!${NC}"
        fi
    fi
    echo ""
else
    echo -e "${YELLOW}[3/3] Skipping embeddings (--skip-embeddings)${NC}"
    EMBEDDINGS_OK=true
    echo ""
fi

# Summary
echo -e "${CYAN}======================================================${NC}"
echo -e "${CYAN}                    Summary                           ${NC}"
echo -e "${CYAN}======================================================${NC}"
echo ""

if [ "$FLORENCE_OK" = true ]; then
    echo -e "  Florence-2-large:    ${GREEN}OK${NC}"
else
    echo -e "  Florence-2-large:    ${RED}FAILED${NC}"
fi

if [ "$OCR_OK" = true ]; then
    echo -e "  PaddleOCR English:   ${GREEN}OK${NC}"
else
    echo -e "  PaddleOCR English:   ${RED}FAILED${NC}"
fi

if [ "$EMBEDDINGS_OK" = true ]; then
    echo -e "  all-MiniLM-L6-v2:    ${GREEN}OK${NC}"
else
    echo -e "  all-MiniLM-L6-v2:    ${RED}FAILED${NC}"
fi

echo ""

# Show disk usage
echo -e "${BLUE}Model disk usage:${NC}"
du -sh "${PROJECT_ROOT}/models/"* 2>/dev/null | sort -h || echo "  (unable to calculate)"
echo ""

# Final status
if [ "$FLORENCE_OK" = true ] && [ "$OCR_OK" = true ] && [ "$EMBEDDINGS_OK" = true ]; then
    echo -e "${GREEN}======================================================${NC}"
    echo -e "${GREEN}  All models downloaded successfully!                 ${NC}"
    echo -e "${GREEN}======================================================${NC}"
    echo ""
    echo "Your node is ready to handle:"
    echo "  - Image description (Florence-2)"
    echo "  - Text extraction from images (PaddleOCR)"
    echo "  - Document embeddings for RAG (all-MiniLM-L6-v2)"
    echo ""
    echo "Next steps:"
    echo "  1. Download an LLM model (GGUF format) to models/"
    echo "  2. Start the node: ./fabstir-llm-node"
    echo ""
    exit 0
else
    echo -e "${RED}======================================================${NC}"
    echo -e "${RED}  Some models failed to download!                     ${NC}"
    echo -e "${RED}======================================================${NC}"
    echo ""
    echo "Please check the error messages above and try again."
    echo "You can re-run specific downloads:"
    echo "  ./scripts/download_florence_model.sh"
    echo "  ./scripts/download_ocr_models.sh"
    echo "  ./scripts/download_embedding_model.sh"
    echo ""
    exit 1
fi
