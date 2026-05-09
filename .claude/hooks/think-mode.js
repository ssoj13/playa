#!/usr/bin/env node

/**
 * think-mode.js
 *
 * UserPromptSubmit 훅: "think" 키워드 감지시 깊은 사고 모드 활성화
 *
 * 키워드:
 * - think, thinking: 기본 사고 모드
 * - ultrathink: 최대 사고 모드 (32k 토큰)
 * - 생각해, 깊이 생각: 한국어 지원
 *
 * 설정 방법 (settings.json):
 * {
 *   "hooks": {
 *     "UserPromptSubmit": [{
 *       "matcher": "*",
 *       "hooks": [{
 *         "type": "command",
 *         "command": "node ~/.claude/hooks/think-mode.js"
 *       }]
 *     }]
 *   }
 * }
 */

const THINK_PATTERNS = {
  ultrathink: {
    patterns: [/\bultrathink\b/i, /\b울트라씽크\b/],
    budget: 32000,
    message: `[ULTRATHINK MODE - 32K TOKENS]
최대 확장 사고 모드가 활성화되었습니다.

이 문제에 대해 깊이 생각하세요:
1. 문제를 여러 각도에서 분석
2. 가능한 모든 접근법 고려
3. 각 접근법의 장단점 평가
4. 엣지 케이스와 잠재적 문제 식별
5. 최적의 솔루션 도출

시간을 들여 철저히 분석한 후 답변하세요.`
  },

  think: {
    patterns: [/\bthink\b/i, /\bthinking\b/i, /\b생각해\b/, /\b깊이\s*생각\b/, /\b곰곰이\b/],
    budget: 16000,
    message: `[THINK MODE - 16K TOKENS]
확장 사고 모드가 활성화되었습니다.

이 문제에 대해 신중하게 생각하세요:
1. 요구사항을 명확히 이해
2. 여러 접근법 고려
3. 최선의 방법 선택
4. 구현 전 계획 수립

충분히 생각한 후 답변하세요.`
  }
};

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const result = detectThinkMode(data);
    console.log(JSON.stringify(result));
    process.exit(0);
  } catch (err) {
    console.error('Error:', err.message);
    process.exit(0);
  }
});

function detectThinkMode(data) {
  const { prompt } = data;

  if (!prompt) {
    return {};
  }

  // 코드 블록 제거
  const textWithoutCode = prompt
    .replace(/```[\s\S]*?```/g, '')
    .replace(/`[^`]*`/g, '');

  // ultrathink 먼저 체크 (더 높은 우선순위)
  for (const pattern of THINK_PATTERNS.ultrathink.patterns) {
    if (pattern.test(textWithoutCode)) {
      return {
        hookSpecificOutput: {
          hookEventName: "UserPromptSubmit",
          additionalContext: THINK_PATTERNS.ultrathink.message
        }
      };
    }
  }

  // 일반 think 체크
  for (const pattern of THINK_PATTERNS.think.patterns) {
    if (pattern.test(textWithoutCode)) {
      return {
        hookSpecificOutput: {
          hookEventName: "UserPromptSubmit",
          additionalContext: THINK_PATTERNS.think.message
        }
      };
    }
  }

  return {};
}
