import { useEffect, useMemo, useState } from 'react'

import {
  addAlwaysWrongWord,
  addIgnoredWord,
  applySearchEdit,
  checkRandomPage,
  checkUrl,
  deleteResult,
  exportAlwaysWrongWords,
  exportIgnoredWords,
  getAlwaysWrongWords,
  getAuthStatus,
  getIgnoredWords,
  getResults,
  getSearchContexts,
  getStats,
  ignoreWordInResult,
  loginWithWikipedia,
  logout,
  sandboxCheck,
  searchWikipedia,
} from './api'
import AlwaysWrongWordsManager from './components/AlwaysWrongWordsManager'
import CheckForm from './components/CheckForm'
import LoadingSpinner from './components/LoadingSpinner'
import ResultRow from './components/ResultRow'
import type { ArticleResult, EditCount, SandboxCheckResponse, SearchResult } from './types'

type Section = 'checker' | 'sandbox' | 'search' | 'stats'

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
  const [alwaysWrongWords, setAlwaysWrongWords] = useState<string[]>([])
  const [isLoggedIn, setIsLoggedIn] = useState(false)
  const [oauthConfigured, setOauthConfigured] = useState(false)
  const [wikipediaWordlists, setWikipediaWordlists] = useState(false)
  const [pendingValidExports, setPendingValidExports] = useState(0)
  const [searchQuery, setSearchQuery] = useState('')
  const [searchTerm, setSearchTerm] = useState('')
  const [searchResults, setSearchResults] = useState<SearchResult[]>([])
  const [searchLimit, setSearchLimit] = useState(50)
  const [searchOffset, setSearchOffset] = useState(0)
  const [loadingMore, setLoadingMore] = useState(false)
  const [stats, setStats] = useState<EditCount[]>([])
  const [statsLoading, setStatsLoading] = useState(false)

  useEffect(() => {
    void (async () => {
      try {
        const [initial, ignoredResponse, authStatus, alwaysWrongResponse] = await Promise.all([
          getResults(),
          getIgnoredWords(),
          getAuthStatus(),
          getAlwaysWrongWords(),
        ])
        setResults(initial)
        setIgnoredWords(ignoredResponse.words)
        setAlwaysWrongWords(alwaysWrongResponse.words)
        setIsLoggedIn(authStatus.logged_in)
        setOauthConfigured(authStatus.oauth_configured)
        setWikipediaWordlists(authStatus.wikipedia_wordlists)

        const params = new URLSearchParams(window.location.search)
        if (params.get('auth') === 'success') {
          setSuccess('Logged in to Wikipedia successfully')
          window.history.replaceState({}, '', window.location.pathname)
        } else if (params.get('auth') === 'not_autoconfirmed') {
          setError('Your Wikipedia account must be autoconfirmed (≈50 edits and 4+ days old) to log in here.')
          window.history.replaceState({}, '', window.location.pathname)
        } else if (params.get('auth') === 'error') {
          setError('Wikipedia login failed. Please try again.')
          window.history.replaceState({}, '', window.location.pathname)
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load data')
      }
    })()
  }, [])

  // Refresh the leaderboard each time the Stats tab is opened so counts stay current.
  useEffect(() => {
    if (section !== 'stats') return
    setStatsLoading(true)
    void getStats()
      .then(setStats)
      .catch((err) => setError(err instanceof Error ? err.message : 'Failed to load stats'))
      .finally(() => setStatsLoading(false))
  }, [section])

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
      const nowEmpty = results.filter(
        (entry) => entry.wrong_words.filter((w) => w !== normalized).length === 0,
      )
      await Promise.all(nowEmpty.map((entry) => deleteResult(entry.id)))
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
      setPendingValidExports((prev) => prev + 1)
      setSuccess(`"${normalized}" was added as a valid word`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save valid word')
    }
  }

  const handleIgnoreWord = async (resultId: number, word: string) => {
    setError(null)
    try {
      await ignoreWordInResult(resultId, word)
      setResults((prev) =>
        prev
          .map((entry) =>
            entry.id === resultId
              ? { ...entry, wrong_words: entry.wrong_words.filter((w) => w !== word) }
              : entry,
          )
          .filter((entry) => entry.wrong_words.length > 0),
      )
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to ignore word')
    }
  }

  const handleLogout = async () => {
    try {
      await logout()
      setIsLoggedIn(false)
      setSuccess('Logged out from Wikipedia')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Logout failed')
    }
  }

  const handleExportIgnoredWords = async () => {
    setLoading(true)
    setError(null)
    setSuccess(null)
    try {
      const response = await exportIgnoredWords()
      setPendingValidExports(0)
      if (wikipediaWordlists) {
        setSuccess(`Exported ${response.exported_count} valid words to ${response.path}`)
      } else {
        setSuccess(`Exported ${response.exported_count} ignored words to ${response.path}`)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to export ignored words')
    } finally {
      setLoading(false)
    }
  }

  const handleAddAlwaysWrongWord = async (word: string) => {
    setError(null)
    try {
      await addAlwaysWrongWord(word)
      setAlwaysWrongWords((prev) => {
        if (prev.includes(word)) return prev
        return [...prev, word].sort((a, b) => a.localeCompare(b))
      })
      setSuccess(`"${word}" will now always be flagged as an error`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to add always wrong word')
    }
  }

  const handleExportAlwaysWrongWords = async () => {
    setError(null)
    try {
      const response = await exportAlwaysWrongWords()
      setSuccess(`Exported ${response.exported_count} always wrong words to ${response.path}`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to export always wrong words')
    }
  }

  const handleSearchSubmit = async () => {
    const query = searchQuery.trim()
    if (!query) return
    setLoading(true)
    setError(null)
    setSuccess(null)
    try {
      const results = await searchWikipedia(query, searchLimit, 0)
      setSearchResults(results)
      setSearchTerm(query)
      setSearchOffset(searchLimit)
      if (results.length === 0) {
        setSuccess(`No results found for "${query}"`)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Search request failed')
    } finally {
      setLoading(false)
    }
  }

  const handleLoadMore = async () => {
    setLoadingMore(true)
    setError(null)
    try {
      const results = await searchWikipedia(searchTerm, searchLimit, searchOffset)
      setSearchResults((prev) => {
        const existingUrls = new Set(prev.map((r) => r.url))
        return [...prev, ...results.filter((r) => !existingUrls.has(r.url))]
      })
      setSearchOffset((prev) => prev + searchLimit)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Search request failed')
    } finally {
      setLoadingMore(false)
    }
  }

  return (
    <main className="mx-auto min-h-screen w-full max-w-4xl p-4 sm:p-6">
      {loading && <LoadingSpinner label="Checking article orthography..." />}

      <section className="mb-6">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="mb-1 text-2xl font-bold text-slate-900">Orthonaut</h1>
            <p className="text-sm text-slate-600">
              Check Spanish orthography from Wikipedia URLs or manual text/HTML sandbox input.
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {oauthConfigured ? (
              isLoggedIn ? (
                <button
                  type="button"
                  onClick={() => void handleLogout()}
                  className="rounded-md bg-slate-200 px-3 py-1.5 text-sm font-medium text-slate-700 transition hover:bg-slate-300"
                >
                  Logout Wikipedia
                </button>
              ) : (
                <button
                  type="button"
                  onClick={loginWithWikipedia}
                  className="rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-blue-700"
                >
                  Login with Wikipedia
                </button>
              )
            ) : (
              <span
                title="Add [oauth] section to orthonaut.toml to enable editing"
                className="cursor-help rounded-md bg-slate-100 px-3 py-1.5 text-sm text-slate-400"
              >
                Editing not configured
              </span>
            )}
          </div>
        </div>
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
          <a
            href="#search"
            onClick={(event) => {
              event.preventDefault()
              setSection('search')
            }}
            className={`rounded-md px-3 py-1.5 text-sm font-medium transition ${
              section === 'search' ? 'bg-blue-600 text-white' : 'bg-slate-200 text-slate-700 hover:bg-slate-300'
            }`}
          >
            Search/Replace
          </a>
          {/* Stats tab is hidden from the UI for now; the `/api/stats` endpoint stays available. */}
        </div>
      </section>

      {section === 'checker' && (
        <>
          <section className="mb-4">
            <CheckForm
              url={url}
              loading={loading}
              onUrlChange={setUrl}
              onSubmit={handleSubmit}
              onAnalyzeRandom={handleRandomSubmit}
            />
            <div className="mt-3 flex flex-col gap-3">
              {wikipediaWordlists ? (
                <button
                  type="button"
                  onClick={() => void handleExportIgnoredWords()}
                  disabled={loading || !isLoggedIn || pendingValidExports === 0}
                  title={
                    isLoggedIn
                      ? 'Exporta las nuevas palabras válidas a la página oficial de Wikipedia.'
                      : 'Inicia sesión en Wikipedia para exportar las palabras válidas.'
                  }
                  className="self-start rounded-md bg-slate-800 px-3 py-2 text-sm font-medium text-white transition hover:bg-slate-900 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  Export valid words to Wikipedia
                  {pendingValidExports > 0 ? ` (${pendingValidExports})` : ''}
                </button>
              ) : (
                <>
                  <button
                    type="button"
                    onClick={() => void handleExportIgnoredWords()}
                    disabled={loading}
                    className="self-start rounded-md bg-slate-800 px-3 py-2 text-sm font-medium text-white transition hover:bg-slate-900 disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    Export valid words to file
                  </button>
                  {/* Manual wrong-word management is a local-dev convenience; in
                      production (Toolforge) wrong words come from the Wikipedia page. */}
                  {import.meta.env.DEV && (
                    <AlwaysWrongWordsManager
                      count={alwaysWrongWords.length}
                      onAdd={handleAddAlwaysWrongWord}
                      onExport={handleExportAlwaysWrongWords}
                    />
                  )}
                </>
              )}
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
                  onIgnore={(word) => void handleIgnoreWord(result.id, word)}
                  ignoredWords={ignoredWordsSet}
                  isLoggedIn={isLoggedIn}
                  oauthConfigured={oauthConfigured}
                />
              ))
            )}
          </section>
        </>
      )}

      {section === 'sandbox' && (
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
                        disabled={!isLoggedIn}
                        title={isLoggedIn ? undefined : 'Log in to Wikipedia to mark words as valid.'}
                        className="rounded bg-emerald-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-emerald-700 disabled:cursor-not-allowed disabled:bg-slate-300 disabled:text-slate-500 disabled:hover:bg-slate-300"
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

      {section === 'search' && (
        <>
          <section className="mb-4 rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
            <label htmlFor="search-input" className="mb-2 block text-sm font-medium text-slate-800">
              Search term (exact match)
            </label>
            <div className="flex gap-2">
              <input
                id="search-input"
                type="text"
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
                onKeyDown={(event) => { if (event.key === 'Enter') void handleSearchSubmit() }}
                placeholder="e.g. categoria"
                className="flex-1 rounded-md border border-slate-300 p-2 text-sm outline-none ring-blue-500 transition focus:ring-2"
                disabled={loading}
              />
              <select
                value={searchLimit}
                onChange={(event) => setSearchLimit(Number(event.target.value))}
                disabled={loading}
                className="rounded-md border border-slate-300 p-2 text-sm outline-none ring-blue-500 transition focus:ring-2"
                title="Number of Wikipedia pages to analyze"
              >
                {[10, 20, 50, 100, 200].map((n) => (
                  <option key={n} value={n}>{n} pages</option>
                ))}
              </select>
              <button
                type="button"
                onClick={() => void handleSearchSubmit()}
                disabled={loading || searchQuery.trim().length === 0}
                className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                Search Wikipedia
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
            {searchResults.map((result) => (
              <ResultRow
                key={result.url}
                result={{
                  id: 0,
                  title: result.title,
                  url: result.url,
                  revision_id: '',
                  wrong_words: [searchTerm],
                  checked_at: '',
                }}
                onDelete={() => {
                  setSearchResults((prev) => prev.filter((r) => r.url !== result.url))
                  return Promise.resolve()
                }}
                isLoggedIn={isLoggedIn}
                oauthConfigured={oauthConfigured}
                onFetchContexts={(word) => getSearchContexts(result.url, word)}
                onApplyEdit={(word, replacement, occurrenceIndex) =>
                  applySearchEdit(result.url, word, replacement, occurrenceIndex)
                }
              />
            ))}
          </section>

          {searchTerm && (
            <div className="mt-4 flex justify-center">
              <button
                type="button"
                onClick={() => void handleLoadMore()}
                disabled={loadingMore}
                className="rounded-md border border-slate-300 bg-white px-6 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {loadingMore ? 'Loading…' : `Load more (next ${searchLimit} pages)`}
              </button>
            </div>
          )}
        </>
      )}

      {section === 'stats' && (
        <section className="mb-4 rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
          <h2 className="mb-1 text-lg font-semibold text-slate-800">Ediciones por usuario</h2>
          <p className="mb-4 text-sm text-slate-500">
            Número de ediciones realizadas con esta aplicación, de mayor a menor.
          </p>
          {statsLoading ? (
            <LoadingSpinner />
          ) : stats.length === 0 ? (
            <p className="text-sm text-slate-600">Aún no hay ediciones registradas.</p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm text-slate-700">
                <thead className="border-b border-slate-200 text-left text-slate-500">
                  <tr>
                    <th className="px-3 py-2 font-medium">Usuario</th>
                    <th className="px-3 py-2 text-right font-medium">Ediciones</th>
                  </tr>
                </thead>
                <tbody>
                  {stats.map((stat) => (
                    <tr key={stat.username} className="border-b border-slate-100 hover:bg-slate-50">
                      <td className="px-3 py-2">
                        <a
                          href={`https://es.wikipedia.org/wiki/Usuario:${encodeURIComponent(stat.username)}`}
                          target="_blank"
                          rel="noreferrer"
                          className="text-blue-700 hover:underline"
                        >
                          {stat.username}
                        </a>
                      </td>
                      <td className="px-3 py-2 text-right font-semibold tabular-nums">{stat.edit_count}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>
      )}
    </main>
  )
}

export default App
