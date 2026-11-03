# Contoh payload `proceed` (room uji 823944, disensor sebagian)

## MCQ
```json
{"roomHash":"6aea1d2fc3cd1f1335eb72fd","playerId":"WayBotProbe","response":{"attempt":0,"questionId":"6ada4a37479817ef3bcad18d","questionType":"MCQ","response":1,"responseType":"original","timeTaken":9404,"answer":[],"isEvaluated":false,"state":"attempted","provisional":{"scores":{"correct":600,"incorrect":0},"scoreBreakups":{"correct":{"base":600,"timer":0,"streak":0,"total":600,"powerups":[]},"incorrect":{"base":0,"timer":0,"streak":0,"total":0,"powerups":[]}},"teamAdjustments":{"correct":0,"incorrect":0}}},"questionId":"…","powerupEffects":{"destroy":[]},"quizVersionId":"…","elapsed":0,"isLastPlayerResponse":false}
```
Respons server: `marks:1`, `metadata.wrongIndices:null`.

## MSQ
`response` berupa array indeks, mis. `[3,1]`.

## BLANK
```json
{"questionType":"BLANK","response":{"media":null},"answer":[{"type":"BlankTargetObject","descriptor":"Answer","value":[{"targetId":"fc503250125b","value":{"text":"Central Processing Unit"}}]}]}
```

## OPEN
```json
{"questionType":"OPEN","response":{"version":"2.0","text":"<p></p><p>Sistem operasi mengelola perangkat keras…</p>","media":null,"sttResponse":null},"answer":[]}
```

## Kunci di quiz data
- MCQ: `structure.answer = 1`
- MSQ: `structure.answer = [0,2,3]`
- BLANK: `structure.answer = [{"targetId":"…","optionId":["…"]}]`, teks di `structure.options[]`.
