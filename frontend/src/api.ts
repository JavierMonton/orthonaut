import type { ArticleResult, CheckResponse, IgnoredWordsResponse, SandboxCheckResponse } from './types'

export async function getResults(): Promise<ArticleResult[]> {
  const response = await fetch('/api/results')
  if (!response.ok) {
    throw new Error('Failed to load results')
  }
  return response.json()
}

export async function checkUrl(url: string): Promise<CheckResponse> {
  const response = await fetch('/api/check', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url }),
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to check URL')
  }
  return response.json()
}

export async function deleteResult(id: number): Promise<void> {
  const response = await fetch(`/api/results/${id}`, {
    method: 'DELETE',
  })
  if (!response.ok) {
    throw new Error('Failed to delete result')
  }
}

export async function sandboxCheck(content: string): Promise<SandboxCheckResponse> {
  const response = await fetch('/api/sandbox/check', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content }),
  })

  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to check sandbox content')
  }

  return response.json()
}

export async function getIgnoredWords(): Promise<IgnoredWordsResponse> {
  const response = await fetch('/api/ignored-words')
  if (!response.ok) {
    throw new Error('Failed to load ignored words')
  }
  return response.json()
}

export async function addIgnoredWord(word: string): Promise<void> {
  const response = await fetch('/api/ignored-words', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ word }),
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to add ignored word')
  }
}

export async function deleteIgnoredWord(word: string): Promise<void> {
  const response = await fetch(`/api/ignored-words/${encodeURIComponent(word)}`, {
    method: 'DELETE',
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to remove ignored word')
  }
}
