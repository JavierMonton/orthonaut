import type {
  ApplyEditResponse,
  ArticleResult,
  AuthStatusResponse,
  CheckResponse,
  ExportIgnoredWordsResponse,
  IgnoredWordsResponse,
  SandboxCheckResponse,
  SearchResult,
  WordContextsResponse,
} from './types'

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

export async function checkRandomPage(): Promise<CheckResponse> {
  const response = await fetch('/api/check/random', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({}),
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to check random page')
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

export async function ignoreWordInResult(id: number, word: string): Promise<void> {
  const response = await fetch(`/api/results/${id}/words/${encodeURIComponent(word)}`, {
    method: 'DELETE',
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to ignore word')
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

export async function exportIgnoredWords(): Promise<ExportIgnoredWordsResponse> {
  const response = await fetch('/api/ignored-words/export', {
    method: 'POST',
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to export ignored words')
  }
  return response.json()
}

export async function getWordContexts(id: number, word: string): Promise<WordContextsResponse> {
  const response = await fetch(`/api/results/${id}/contexts/${encodeURIComponent(word)}`)
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to load word contexts')
  }
  return response.json()
}

export async function getAuthStatus(): Promise<AuthStatusResponse> {
  const response = await fetch('/api/auth/status')
  if (!response.ok) {
    throw new Error('Failed to get auth status')
  }
  return response.json()
}

export function loginWithWikipedia(): void {
  window.location.href = '/api/auth/login'
}

export async function logout(): Promise<void> {
  const response = await fetch('/api/auth/logout', { method: 'POST' })
  if (!response.ok) {
    throw new Error('Failed to log out')
  }
}

export async function applyEdit(
  articleId: number,
  word: string,
  replacement: string,
  occurrenceIndex?: number,
): Promise<ApplyEditResponse> {
  const response = await fetch('/api/edit', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ article_id: articleId, word, replacement, occurrence_index: occurrenceIndex }),
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to apply edit')
  }
  return response.json()
}

export async function searchWikipedia(query: string, limit = 50, offset = 0): Promise<SearchResult[]> {
  const response = await fetch('/api/search', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, limit, offset }),
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to search Wikipedia')
  }
  return response.json()
}

export async function getSearchContexts(url: string, word: string): Promise<WordContextsResponse> {
  const response = await fetch('/api/search/contexts', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url, word }),
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to load search contexts')
  }
  return response.json()
}

export async function applySearchEdit(
  url: string,
  word: string,
  replacement: string,
  occurrenceIndex?: number,
): Promise<ApplyEditResponse> {
  const response = await fetch('/api/search/edit', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url, word, replacement, occurrence_index: occurrenceIndex }),
  })
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}))
    throw new Error(payload.error ?? 'Failed to apply search edit')
  }
  return response.json()
}
