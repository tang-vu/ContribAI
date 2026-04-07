# Vertex AI Integration Reference

> Extracted from ContribAI project — working implementation for Google Vertex AI (Gemini models).

## 1. Authentication

Vertex AI uses **gcloud CLI** for authentication instead of API keys:

```bash
# Login once
gcloud auth login
gcloud auth application-default login

# Set active project
gcloud config set project YOUR_PROJECT_ID

# Test token retrieval
gcloud auth print-access-token
```

**No API key needed** — authentication is handled by `gcloud auth print-access-token`.

---

## 2. Configuration

```yaml
llm:
  provider: "gemini"          # or "vertex"
  model: "gemini-3-flash-preview"
  vertex_project: "YOUR_GCP_PROJECT_ID"
  vertex_location: "global"   # or specific region like "us-central1"
  api_key: ""                 # Leave empty for Vertex AI
```

Environment variables:
```bash
export GOOGLE_CLOUD_PROJECT=YOUR_PROJECT_ID
```

---

## 3. Endpoint URLs

### API Key Mode (Standard Gemini)
```
https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={API_KEY}
```

### Vertex AI Mode
```
https://{HOSTNAME}/{API_VERSION}/projects/{PROJECT}/locations/{LOCATION}/publishers/google/models/{MODEL}:generateContent
```

Where:
- `HOSTNAME` = `aiplatform.googleapis.com` (if location is "global")
- `HOSTNAME` = `{LOCATION}-aiplatform.googleapis.com` (for regional endpoints)
- `API_VERSION` = `v1beta1` (for preview models) or `v1` (for stable models)

**Examples:**
```
# Global endpoint
https://aiplatform.googleapis.com/v1beta1/projects/my-project/locations/global/publishers/google/models/gemini-3-flash-preview:generateContent

# Regional endpoint (us-central1)
https://us-central1-aiplatform.googleapis.com/v1/v1/projects/my-project/locations/us-central1/publishers/google/models/gemini-2.0-flash:generateContent
```

---

## 4. Request Body

```json
{
  "contents": [{
    "role": "user",
    "parts": [{ "text": "Your prompt here" }]
  }],
  "generationConfig": {
    "temperature": 0.3,
    "maxOutputTokens": 65536
  },
  "systemInstruction": {
    "parts": [{ "text": "Optional system prompt" }]
  }
}
```

---

## 5. Authentication Flow

### Token Fetching (Rust implementation)

**Windows:**
```rust
let out = std::process::Command::new("cmd")
    .args(["/c", "gcloud", "auth", "print-access-token"])
    .output()?;
```

**Linux/Mac:**
```rust
let out = std::process::Command::new("gcloud")
    .args(["auth", "print-access-token"])
    .output()?;
```

**Token handling:**
```bash
# Get token
TOKEN=$(gcloud auth print-access-token)

# Use in request
curl -X POST "https://aiplatform.googleapis.com/v1beta1/projects/YOUR_PROJECT/locations/global/publishers/google/models/gemini-3-flash-preview:generateContent" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "role": "user",
      "parts": [{"text": "Hello"}]
    }],
    "generationConfig": {
      "temperature": 0.3,
      "maxOutputTokens": 1024
    }
  }'
```

---

## 6. Complete cURL Example

```bash
# Variables
PROJECT="YOUR_GCP_PROJECT_ID"
LOCATION="global"
MODEL="gemini-3-flash-preview"
API_VERSION="v1beta1"  # Use "v1" for stable models

# Get token
TOKEN=$(gcloud auth print-access-token)

# Build URL
if [ "$LOCATION" = "global" ]; then
  HOSTNAME="aiplatform.googleapis.com"
else
  HOSTNAME="${LOCATION}-aiplatform.googleapis.com"
fi

URL="https://${HOSTNAME}/${API_VERSION}/projects/${PROJECT}/locations/${LOCATION}/publishers/google/models/${MODEL}:generateContent"

# Make request
curl -X POST "$URL" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "role": "user",
      "parts": [{"text": "Explain quantum computing in simple terms"}]
    }],
    "generationConfig": {
      "temperature": 0.3,
      "maxOutputTokens": 2048
    }
  }'
```

---

## 7. Python Implementation

```python
import subprocess
import requests
import json

def get_vertex_token():
    """Fetch access token from gcloud CLI."""
    result = subprocess.run(
        ["gcloud", "auth", "print-access-token"],
        capture_output=True, text=True, check=True
    )
    return result.stdout.strip()

def vertex_ai_complete(prompt, system=None, model="gemini-3-flash-preview",
                       project="YOUR_PROJECT", location="global",
                       temperature=0.3, max_tokens=65536):
    """Call Vertex AI Gemini model."""
    
    # Get token
    token = get_vertex_token()
    
    # Build endpoint
    api_version = "v1beta1" if "preview" in model else "v1"
    
    if location == "global":
        hostname = "aiplatform.googleapis.com"
    else:
        hostname = f"{location}-aiplatform.googleapis.com"
    
    url = f"https://{hostname}/{api_version}/projects/{project}/locations/{location}/publishers/google/models/{model}:generateContent"
    
    # Build request body
    body = {
        "contents": [{
            "role": "user",
            "parts": [{"text": prompt}]
        }],
        "generationConfig": {
            "temperature": temperature,
            "maxOutputTokens": max_tokens
        }
    }
    
    if system:
        body["systemInstruction"] = {
            "parts": [{"text": system}]
        }
    
    # Make request
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json"
    }
    
    response = requests.post(url, json=body, headers=headers)
    response.raise_for_status()
    
    # Extract text
    data = response.json()
    return data["candidates"][0]["content"]["parts"][0]["text"]

# Usage
result = vertex_ai_complete(
    prompt="What is Rust?",
    system="You are a helpful coding assistant.",
    project="my-gcp-project"
)
print(result)
```

---

## 8. Common Issues & Solutions

### Issue: "gcloud not found"
**Solution:** Install Google Cloud SDK:
```bash
# Windows
# Download from https://cloud.google.com/sdk/docs/install

# Linux
curl https://sdk.cloud.google.com | bash
```

### Issue: "Token fetch failed"
**Solution:** Login and set project:
```bash
gcloud auth login
gcloud auth application-default login
gcloud config set project YOUR_PROJECT
```

### Issue: "401 Unauthorized"
**Causes:**
- Token expired (tokens valid for ~1 hour)
- Wrong project ID
- Vertex AI API not enabled in GCP console

**Solution:**
```bash
# Re-authenticate
gcloud auth login

# Enable Vertex AI API
gcloud services enable aiplatform.googleapis.com
```

### Issue: "403 Permission Denied"
**Solution:** Grant Vertex AI User role:
```bash
gcloud projects add-iam-policy-binding YOUR_PROJECT \
  --member="user:YOUR_EMAIL" \
  --role="roles/aiplatform.user"
```

### Issue: Model not found
**Available models:**
- `gemini-3-flash-preview` (v1beta1)
- `gemini-3-pro-preview` (v1beta1)
- `gemini-2.0-flash` (v1)
- `gemini-1.5-pro` (v1)

---

## 9. Context Caching (Advanced)

Vertex AI supports content caching for repeated context:

```bash
# Create cached content
curl -X POST "https://aiplatform.googleapis.com/v1beta1/projects/$PROJECT/locations/$LOCATION/cachedContents" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "projects/$PROJECT/locations/$LOCATION/publishers/google/models/$MODEL",
    "contents": [{
      "role": "user",
      "parts": [{"text": "Your large context text here"}]
    }],
    "ttl": "3600s"
  }'

# Response includes "name": "cachedContents/xyz123"
# Use this in subsequent requests:
# "cachedContent": "cachedContents/xyz123"
```

---

## 10. Key Differences: API Key vs Vertex AI

| Aspect | API Key | Vertex AI |
|--------|---------|-----------|
| Auth | `?key=API_KEY` | `Authorization: Bearer TOKEN` |
| Base URL | `generativelanguage.googleapis.com` | `{location}-aiplatform.googleapis.com` |
| Model Format | `models/gemini-3-flash-preview` | `projects/.../publishers/google/models/...` |
| Token Source | Static key | `gcloud auth print-access-token` |
| Rate Limits | Lower | Higher (enterprise) |
| Caching | Limited | Full context caching |

---

## 11. Quick Checklist

- [ ] gcloud CLI installed
- [ ] `gcloud auth login` completed
- [ ] `gcloud config set project YOUR_PROJECT` done
- [ ] Vertex AI API enabled in GCP Console
- [ ] User has `roles/aiplatform.user` permission
- [ ] `vertex_project` set in config (not empty)
- [ ] `api_key` left empty (or ignored when vertex_project is set)
- [ ] Model name correct (check for "preview" suffix)
- [ ] API version correct (v1beta1 for preview, v1 for stable)

---

## Source

Extracted from: `crates/contribai-rs/src/llm/provider.rs`
Working implementation: Lines 70-300, 1285-1305
Config: `crates/contribai-rs/src/core/config.rs` Lines 240-269
