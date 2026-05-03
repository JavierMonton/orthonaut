export type ArticleResult = {
  id: number
  title: string
  url: string
  revision_id: string
  wrong_words: string[]
  checked_at: string
}

export type CheckResponse = {
  status: 'ok' | 'errors'
  result: ArticleResult | null
  message: string | null
}

export type SandboxCheckResponse = {
  status: 'ok'
  wrong_words: string[]
  total_words: number
  misspelled_count: number
}

export type IgnoredWordsResponse = {
  words: string[]
}
