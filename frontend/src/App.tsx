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
import { DOCUMENTATION_URL } from './constants'
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
          setSuccess('Sesión iniciada en Wikipedia correctamente')
          window.history.replaceState({}, '', window.location.pathname)
        } else if (params.get('auth') === 'not_autoconfirmed') {
          setError('Tu cuenta de Wikipedia debe estar autoconfirmada (≈50 ediciones y más de 4 días de antigüedad) para iniciar sesión aquí.')
          window.history.replaceState({}, '', window.location.pathname)
        } else if (params.get('auth') === 'error') {
          setError('Error al iniciar sesión en Wikipedia. Inténtalo de nuevo.')
          window.history.replaceState({}, '', window.location.pathname)
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'No se pudieron cargar los datos')
      }
    })()
  }, [])

  // Refresh the leaderboard each time the Stats tab is opened so counts stay current.
  useEffect(() => {
    if (section !== 'stats') return
    void (async () => {
      setStatsLoading(true)
      try {
        setStats(await getStats())
      } catch (err) {
        setError(err instanceof Error ? err.message : 'No se pudieron cargar las estadísticas')
      } finally {
        setStatsLoading(false)
      }
    })()
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
      setError(err instanceof Error ? err.message : 'Falló la solicitud de revisión')
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
      setError(err instanceof Error ? err.message : 'Falló la solicitud de revisión aleatoria')
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
        setSuccess('No se encontraron errores ortográficos en el contenido del espacio de pruebas')
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Falló la solicitud de revisión del espacio de pruebas')
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
      setError(err instanceof Error ? err.message : 'Falló la eliminación')
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
      setSuccess(`"${normalized}" se añadió como palabra válida`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'No se pudo guardar la palabra válida')
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
      setError(err instanceof Error ? err.message : 'No se pudo ignorar la palabra')
    }
  }

  const handleLogout = async () => {
    try {
      await logout()
      setIsLoggedIn(false)
      setSuccess('Sesión cerrada en Wikipedia')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Falló el cierre de sesión')
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
        setSuccess(`Se exportaron ${response.exported_count} palabras válidas a ${response.path}`)
      } else {
        setSuccess(`Se exportaron ${response.exported_count} palabras ignoradas a ${response.path}`)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'No se pudieron exportar las palabras ignoradas')
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
      setSuccess(`"${word}" se marcará siempre como un error`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'No se pudo añadir la palabra siempre incorrecta')
    }
  }

  const handleExportAlwaysWrongWords = async () => {
    setError(null)
    try {
      const response = await exportAlwaysWrongWords()
      setSuccess(`Se exportaron ${response.exported_count} palabras siempre incorrectas a ${response.path}`)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'No se pudieron exportar las palabras siempre incorrectas')
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
        setSuccess(`No se encontraron resultados para "${query}"`)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Falló la solicitud de búsqueda')
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
      setError(err instanceof Error ? err.message : 'Falló la solicitud de búsqueda')
    } finally {
      setLoadingMore(false)
    }
  }

  return (
    <div className="flex min-h-screen flex-col">
    <main className="mx-auto w-full max-w-4xl flex-1 p-4 sm:p-6">
      {loading && <LoadingSpinner label="Revisando la ortografía del artículo..." />}

      <section className="mb-6">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="mb-1 text-2xl font-bold text-slate-900">Orthonaut</h1>
            <p className="text-sm text-slate-600">
              Revisa la ortografía en español desde URLs de Wikipedia o introduciendo texto/HTML en el espacio de pruebas.
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
                  Cerrar sesión en Wikipedia
                </button>
              ) : (
                <button
                  type="button"
                  onClick={loginWithWikipedia}
                  className="rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-blue-700"
                >
                  Iniciar sesión con Wikipedia
                </button>
              )
            ) : (
              <span
                title="Añade la sección [oauth] a orthonaut.toml para habilitar la edición"
                className="cursor-help rounded-md bg-slate-100 px-3 py-1.5 text-sm text-slate-400"
              >
                Edición no configurada
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
            Revisor
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
            Buscar/Reemplazar
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
            Pruebas
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
                  Exportar palabras válidas a Wikipedia
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
                    Exportar palabras válidas a archivo
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
                Aún no hay resultados de ortografía guardados.
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
              Pega texto plano o HTML
            </label>
            <textarea
              id="sandbox-input"
              value={sandboxInput}
              onChange={(event) => setSandboxInput(event.target.value)}
              placeholder="<p>Texto para revisar ortografía...</p>"
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
                Revisar contenido del espacio de pruebas
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
                Palabras totales: <span className="font-semibold">{sandboxResult.total_words}</span> - Incorrectas:{' '}
                <span className="font-semibold">{visibleSandboxWords.length}</span>
              </p>
              {visibleSandboxWords.length === 0 ? (
                <p className="text-sm text-slate-600">No se encontraron problemas de ortografía.</p>
              ) : (
                <ul className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                  {visibleSandboxWords.map((word) => (
                    <li key={word} className="flex items-center justify-between gap-3 rounded bg-slate-100 px-2 py-1 text-sm text-slate-700">
                      <span>{word}</span>
                      <button
                        type="button"
                        onClick={() => void handleMarkValidWord(word)}
                        disabled={!isLoggedIn}
                        title={isLoggedIn ? undefined : 'Inicia sesión en Wikipedia para marcar palabras como válidas.'}
                        className="rounded bg-emerald-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-emerald-700 disabled:cursor-not-allowed disabled:bg-slate-300 disabled:text-slate-500 disabled:hover:bg-slate-300"
                      >
                        Palabra válida
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
              Término de búsqueda (coincidencia exacta)
            </label>
            <div className="flex gap-2">
              <input
                id="search-input"
                type="text"
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
                onKeyDown={(event) => { if (event.key === 'Enter') void handleSearchSubmit() }}
                placeholder="p. ej. categoria"
                className="flex-1 rounded-md border border-slate-300 p-2 text-sm outline-none ring-blue-500 transition focus:ring-2"
                disabled={loading}
              />
              <select
                value={searchLimit}
                onChange={(event) => setSearchLimit(Number(event.target.value))}
                disabled={loading}
                className="rounded-md border border-slate-300 p-2 text-sm outline-none ring-blue-500 transition focus:ring-2"
                title="Número de páginas de Wikipedia a analizar"
              >
                {[10, 20, 50, 100, 200].map((n) => (
                  <option key={n} value={n}>{n} páginas</option>
                ))}
              </select>
              <button
                type="button"
                onClick={() => void handleSearchSubmit()}
                disabled={loading || searchQuery.trim().length === 0}
                className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                Buscar en Wikipedia
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
                {loadingMore ? 'Cargando…' : `Cargar más (siguientes ${searchLimit} páginas)`}
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

    <footer className="border-t border-slate-200 bg-slate-200/70 py-6 text-center text-sm text-slate-600">
      <div className="mx-auto w-full max-w-4xl px-4 sm:px-6">
        <a
          href={DOCUMENTATION_URL}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1.5 font-medium text-slate-700 transition hover:text-slate-900 hover:underline"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
            className="h-4 w-4"
            aria-hidden="true"
          >
            <path d="M3 5l3.5 14L12 9l5.5 10L21 5" />
          </svg>
          Documentación
        </a>
      </div>
    </footer>
    </div>
  )
}

export default App
