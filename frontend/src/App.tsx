import { useEffect, useMemo, useState } from 'react'

import {
  addIgnoredWord,
  checkRandomPage,
  checkUrl,
  deleteResult,
  exportIgnoredWords,
  getIgnoredWords,
  getResults,
  sandboxCheck,
} from './api'
import CheckForm from './components/CheckForm'
import LoadingSpinner from './components/LoadingSpinner'
import ResultRow from './components/ResultRow'
import type { ArticleResult, SandboxCheckResponse } from './types'

type Section = 'checker' | 'sandbox'

function App() {
  const [section, setSection] = useState<Section>('checker')
  const [url, setUrl] = useState('')
  const [results, setResults] = useState<ArticleResult[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)
  const [sandboxInput, setSandboxInput] = useState('')
  const [sandboxResult, setSandboxResult] = useState<SandboxCheckResponse | null>(null)
  const [ignoredWords, setIgnoredWords] = useState<string[]>([])

  useEffect(() => {
    void (async () => {
      try {
        const [initial, ignoredResponse] = await Promise.all([getResults(), getIgnoredWords()])
        setResults(initial)
        setIgnoredWords(ignoredResponse.words)
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load data')
      }
    })()
  }, [])

  const sortedResults = useMemo(() => results, [results])
  const ignoredWordsSet = useMemo(() => new Set(ignoredWords), [ignoredWords])
  const visibleSandboxWords = useMemo(
    () => sandboxResult?.wrong_words.filter((word) => !ignoredWordsSet.has(word)) ?? [],
    [sandboxResult, ignoredWordsSet],
  )

  const applyCheckResponse = (response: { status: 'ok' | 'errors'; result: ArticleResult | null; message: string | null }) => {
    if (response.status === 'errors' && response.result) {
      const result = response.result
      const visibleWrongWords = result.wrong_words.filter((word) => !ignoredWordsSet.has(word))
      if (visibleWrongWords.length === 0) {
        setSuccess(response.message ?? `No se encontraron errores claros en ${result.title}`)
      } else {
        setResults((prev) => [{ ...result, wrong_words: visibleWrongWords }, ...prev])
      }
    } else if (response.status === 'ok') {
      setSuccess(response.message ?? 'No se encontraron errores')
    }
  }

  const handleSubmit = async () => {
    setLoading(true)
    setError(null)
    setSuccess(null)
    try {
      const response = await checkUrl(url.trim())
      applyCheckResponse(response)
      setUrl('')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Check request failed')
    } finally {
      setLoading(false)
    }
  }

  const handleRandomSubmit = async () => {
    setLoading(true)
    setError(null)
    setSuccess(null)
    try {
      const response = await checkRandomPage()
      applyCheckResponse(response)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Random check request failed')
    } finally {
      setLoading(false)
    }
  }

  const handleSandboxSubmit = async () => {
    setLoading(true)
    setError(null)
    setSuccess(null)
    try {
      const response = await sandboxCheck(sandboxInput)
      setSandboxResult(response)
      if (response.misspelled_count === 0) {
        setSuccess('No se encontraron errores ortograficos en el contenido del sandbox')
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Sandbox check request failed')
    } finally {
      setLoading(false)
    }
  }

  const handleDelete = async (id: number) => {
    setError(null)
    try {
      await deleteResult(id)
      setResults((prev) => prev.filter((entry) => entry.id !== id))
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Delete failed')
    }
  }

  const handleMarkValidWord = async (word: string) => {
    const normalized = word.trim().toLowerCase()
    if (!normalized) {
      return
    }

    setError(null)
    try {
      await addIgnoredWord(normalized)
      setIgnoredWords((prev) => {
        if (prev.includes(normalized)) {
          return prev
        }
        return [...prev, normalized].sort((a, b) => a.localeCompare(b))
      })
      setResults((prev) =>
        prev
          .map((entry) => ({
            ...entry,
            wrong_words: entry.wrong_words.filter((w) => w !== normalized),
          }))
          .filter((entry) => entry.wrong_words.length > 0),
      )
      setSandboxResult((prev) => {
        if (!prev) {
          return prev
        }
        const filtered = prev.wrong_words.filter((w) => w !== normalized)
        return {
          ...prev,
          wrong_words: filtered,
          misspelled_count: filtered.length,
        }
      })
      setSuccess(`"${normalized}" was added as a valid word`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save valid word')
    }
  }

  const handleExportIgnoredWords = async () => {
    setLoading(true)
    setError(null)
    setSuccess(null)
    try {
      const response = await exportIgnoredWords()
      setSuccess(`Exported ${response.exported_count} ignored words to ${response.path}`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to export ignored words')
    } finally {
      setLoading(false)
    }
  }

  return (
    <main className="mx-auto min-h-screen w-full max-w-4xl p-4 sm:p-6">
      {loading && <LoadingSpinner label="Checking article orthography..." />}

      <section className="mb-6">
        <h1 className="mb-1 text-2xl font-bold text-slate-900">Ortobot</h1>
        <p className="text-sm text-slate-600">
          Check Spanish orthography from Wikipedia URLs or manual text/HTML sandbox input.
        </p>
        <div className="mt-4 flex gap-2">
          <a
            href="#checker"
            onClick={(event) => {
              event.preventDefault()
              setSection('checker')
            }}
            className={`rounded-md px-3 py-1.5 text-sm font-medium transition ${
              section === 'checker' ? 'bg-blue-600 text-white' : 'bg-slate-200 text-slate-700 hover:bg-slate-300'
            }`}
          >
            Checker
          </a>
          <a
            href="#sandbox"
            onClick={(event) => {
              event.preventDefault()
              setSection('sandbox')
            }}
            className={`rounded-md px-3 py-1.5 text-sm font-medium transition ${
              section === 'sandbox' ? 'bg-blue-600 text-white' : 'bg-slate-200 text-slate-700 hover:bg-slate-300'
            }`}
          >
            Sandbox
          </a>
        </div>
      </section>

      {section === 'checker' ? (
        <>
          <section className="mb-4">
            <CheckForm
              url={url}
              loading={loading}
              onUrlChange={setUrl}
              onSubmit={handleSubmit}
              onAnalyzeRandom={handleRandomSubmit}
            />
            <div className="mt-3">
              <button
                type="button"
                onClick={() => void handleExportIgnoredWords()}
                disabled={loading}
                className="rounded-md bg-slate-800 px-3 py-2 text-sm font-medium text-white transition hover:bg-slate-900 disabled:cursor-not-allowed disabled:opacity-60"
              >
                Export ignored words to file
              </button>
            </div>
          </section>

          {error && (
            <div className="mb-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
              {error}
            </div>
          )}

          {success && (
            <div className="mb-4 rounded-md border border-green-200 bg-green-50 px-3 py-2 text-sm text-green-700">
              {success}
            </div>
          )}

          <section className="space-y-3">
            {sortedResults.length === 0 ? (
              <div className="rounded-xl border border-slate-200 bg-white p-4 text-sm text-slate-600 shadow-sm">
                No stored orthography findings yet.
              </div>
            ) : (
              sortedResults.map((result) => (
                <ResultRow
                  key={result.id}
                  result={result}
                  onDelete={handleDelete}
                  onMarkValid={handleMarkValidWord}
                  ignoredWords={ignoredWordsSet}
                />
              ))
            )}
          </section>
        </>
      ) : (
        <>
          <section className="mb-4 rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
            <label htmlFor="sandbox-input" className="mb-2 block text-sm font-medium text-slate-800">
              Paste plain text or HTML
            </label>
            <textarea
              id="sandbox-input"
              value={sandboxInput}
              onChange={(event) => setSandboxInput(event.target.value)}
              placeholder="<p>Texto para revisar ortografia...</p>"
              className="min-h-72 w-full rounded-md border border-slate-300 p-3 text-sm outline-none ring-blue-500 transition focus:ring-2"
              disabled={loading}
            />
            <div className="mt-3">
              <button
                type="button"
                onClick={handleSandboxSubmit}
                disabled={loading || sandboxInput.trim().length === 0}
                className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                Check sandbox content
              </button>
            </div>
          </section>

          {error && (
            <div className="mb-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
              {error}
            </div>
          )}

          {success && (
            <div className="mb-4 rounded-md border border-green-200 bg-green-50 px-3 py-2 text-sm text-green-700">
              {success}
            </div>
          )}

          {sandboxResult && (
            <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
              <p className="mb-2 text-sm text-slate-700">
                Total words: <span className="font-semibold">{sandboxResult.total_words}</span> - Misspelled:{' '}
                <span className="font-semibold">{visibleSandboxWords.length}</span>
              </p>
              {visibleSandboxWords.length === 0 ? (
                <p className="text-sm text-slate-600">No orthography issues found.</p>
              ) : (
                <ul className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                  {visibleSandboxWords.map((word) => (
                    <li key={word} className="flex items-center justify-between gap-3 rounded bg-slate-100 px-2 py-1 text-sm text-slate-700">
                      <span>{word}</span>
                      <button
                        type="button"
                        onClick={() => void handleMarkValidWord(word)}
                        className="rounded bg-emerald-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-emerald-700"
                      >
                        Valid word
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          )}
        </>
      )}
    </main>
  )
}

export default App
