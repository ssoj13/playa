#!/usr/bin/env node

/**
 * keyword-detector.js
 *
 * UserPromptSubmit 훅: 키워드 감지하여 모드 전환
 *
 * 키워드:
 * - ultrawork, ulw: 최대 성능 모드
 * - search, find: 병렬 탐색 모드
 * - analyze, investigate: 심층 분석 모드
 * - parallel: 병렬 에이전트 실행
 *
 * 설정 방법 (settings.json):
 * {
 *   "hooks": {
 *     "UserPromptSubmit": [{
 *       "matcher": "*",
 *       "hooks": [{
 *         "type": "command",
 *         "command": "node ~/.claude/hooks/keyword-detector.js"
 *       }]
 *     }]
 *   }
 * }
 */

// 키워드 정의
const KEYWORDS = {
  ultrawork: {
    patterns: [/\bultrawork\b/i, /\bulw\b/i, /\b울트라워크\b/],
    mode: 'max_performance',
    message: `[ULTRAWORK MODE ACTIVATED]
최대 성능 모드가 활성화되었습니다.
- 병렬 에이전트 실행 활성화
- 깊은 분석 및 탐색 수행
- 작업 완료까지 멈추지 않음

이 작업을 최고 품질로 완수하세요.`
  },

  search: {
    patterns: [/\bsearch\b/i, /\bfind\b/i, /\b검색\b/, /\b찾아\b/],
    mode: 'parallel_search',
    message: `[PARALLEL SEARCH MODE]
병렬 탐색 모드가 활성화되었습니다.
- explore 에이전트로 코드베이스 탐색
- librarian 에이전트로 문서/예시 검색
- 여러 검색을 동시에 실행하세요`
  },

  analyze: {
    patterns: [/\banalyze\b/i, /\binvestigate\b/i, /\b분석\b/, /\b조사\b/],
    mode: 'deep_analysis',
    message: `[DEEP ANALYSIS MODE]
심층 분석 모드가 활성화되었습니다.
- 문제를 여러 각도에서 분석
- oracle 에이전트와 상담 권장
- 근본 원인 파악에 집중`
  },

  parallel: {
    patterns: [/\bparallel\b/i, /\b병렬\b/, /\b동시에\b/],
    mode: 'parallel_agents',
    message: `[PARALLEL EXECUTION - MANDATORY]

⚠️ 이 요청은 반드시 병렬로 처리해야 합니다.

## 필수 규칙
1. **한 메시지에 여러 Task 호출**: 독립 작업은 반드시 동일 메시지에서 여러 Task 도구를 호출
2. **sisyphus 오케스트레이터 사용 권장**: 복잡한 작업은 sisyphus에게 위임
3. **의존성 분석 먼저**: 작업 간 의존성을 파악하고, 독립 작업은 동시 실행

## 병렬 실행 예시
[동시 실행]
├── Task(explore): 코드베이스 탐색
├── Task(librarian): 문서 검색
└── Task(oracle): 아키텍처 검토

[순차 실행 - 위 결과 필요 시]
└── 구현 작업

절대 순차적으로 하나씩 실행하지 마세요.`
  }
};

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const result = detectKeywords(data);
    console.log(JSON.stringify(result));
    process.exit(0);
  } catch (err) {
    console.error('Error:', err.message);
    process.exit(0);
  }
});

function detectKeywords(data) {
  const { prompt } = data;

  if (!prompt) {
    return {};
  }

  // 코드 블록 제거 (코드 내 키워드는 무시)
  const textWithoutCode = prompt
    .replace(/```[\s\S]*?```/g, '')
    .replace(/`[^`]*`/g, '');

  const detectedModes = [];
  const messages = [];

  for (const [name, config] of Object.entries(KEYWORDS)) {
    for (const pattern of config.patterns) {
      if (pattern.test(textWithoutCode)) {
        detectedModes.push(config.mode);
        messages.push(config.message);
        break;
      }
    }
  }

  if (detectedModes.length === 0) {
    return {};
  }

  // 감지된 모드들의 메시지 결합
  const combinedMessage = messages.join('\n\n---\n\n');

  return {
    hookSpecificOutput: {
      hookEventName: "UserPromptSubmit",
      additionalContext: combinedMessage
    }
  };
}
