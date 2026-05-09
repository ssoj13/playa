#!/usr/bin/env node

/**
 * context-monitor.js
 *
 * PostToolUse 훅: 컨텍스트 사용량 모니터링 및 경고
 *
 * 임계값:
 * - 70%: 경고 (서두르지 말 것)
 * - 85%: 심각 경고 (압축 고려)
 *
 * 설정 방법 (settings.json):
 * {
 *   "hooks": {
 *     "PostToolUse": [{
 *       "matcher": "*",
 *       "hooks": [{
 *         "type": "command",
 *         "command": "node ~/.claude/hooks/context-monitor.js"
 *       }]
 *     }]
 *   }
 * }
 */

import fs from 'fs';

// 임계값 설정
const WARNING_THRESHOLD = 0.70;  // 70%
const CRITICAL_THRESHOLD = 0.85; // 85%

// 대략적인 토큰 추정 (실제 토큰화와 다를 수 있음)
const ESTIMATED_MAX_TOKENS = 200000; // Claude의 대략적인 컨텍스트 크기

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const result = monitorContext(data);
    console.log(JSON.stringify(result));
    process.exit(0);
  } catch (err) {
    console.error('Error:', err.message);
    process.exit(0);
  }
});

function monitorContext(data) {
  const { transcript_path, tool_response } = data;

  if (!transcript_path) {
    return {};
  }

  // transcript 파일 크기로 대략적인 사용량 추정
  let estimatedUsage = 0;

  try {
    if (fs.existsSync(transcript_path)) {
      const stats = fs.statSync(transcript_path);
      const fileSizeBytes = stats.size;

      // 대략 4 characters = 1 token (영어 기준, 추정치)
      const estimatedTokens = fileSizeBytes / 4;
      estimatedUsage = estimatedTokens / ESTIMATED_MAX_TOKENS;
    }
  } catch (err) {
    return {};
  }

  // 도구 응답 크기도 고려
  if (tool_response) {
    const responseTokens = tool_response.length / 4;
    const responseRatio = responseTokens / ESTIMATED_MAX_TOKENS;

    // 큰 응답이 왔을 때 경고
    if (responseRatio > 0.05) { // 응답이 5% 이상
      return {
        additionalContext: `[CONTEXT ALERT]
방금 받은 도구 응답이 큽니다 (~${Math.round(responseTokens)} tokens).
필요한 정보만 추출하고 나머지는 요약하세요.`
      };
    }
  }

  // 컨텍스트 사용량 경고
  if (estimatedUsage >= CRITICAL_THRESHOLD) {
    return {
      additionalContext: `[CRITICAL CONTEXT WARNING - ${Math.round(estimatedUsage * 100)}%]
컨텍스트 윈도우가 거의 가득 찼습니다!

즉시 조치:
1. 현재 작업 상태를 TODO에 기록
2. 불필요한 탐색/검색 중단
3. 핵심 작업에만 집중
4. 필요시 /compact 명령으로 압축

작업을 서둘러 마무리하거나 세션을 정리하세요.`
    };
  }

  if (estimatedUsage >= WARNING_THRESHOLD) {
    return {
      additionalContext: `[CONTEXT WARNING - ${Math.round(estimatedUsage * 100)}%]
컨텍스트 사용량이 높습니다.

권장 사항:
- 불필요한 파일 읽기 자제
- 검색 결과는 필요한 것만 사용
- 큰 파일은 필요한 부분만 읽기
- 작업 진행 상황을 TODO에 기록해두세요`
    };
  }

  return {};
}
