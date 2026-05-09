#!/usr/bin/env node

/**
 * comment-checker.js
 *
 * PostToolUse 훅: 파일 수정 후 과도한 주석 감지
 *
 * 설정 방법 (settings.json):
 * {
 *   "hooks": {
 *     "PostToolUse": [{
 *       "matcher": "Edit|Write",
 *       "hooks": [{
 *         "type": "command",
 *         "command": "node ~/.claude/hooks/comment-checker.js"
 *       }]
 *     }]
 *   }
 * }
 */

import fs from 'fs';
import path from 'path';

// 주석 패턴 (언어별)
const COMMENT_PATTERNS = {
  js: [/\/\/.*$/gm, /\/\*[\s\S]*?\*\//g],
  ts: [/\/\/.*$/gm, /\/\*[\s\S]*?\*\//g],
  py: [/#.*$/gm, /'''[\s\S]*?'''/g, /"""[\s\S]*?"""/g],
  rb: [/#.*$/gm, /=begin[\s\S]*?=end/g],
  java: [/\/\/.*$/gm, /\/\*[\s\S]*?\*\//g],
  go: [/\/\/.*$/gm, /\/\*[\s\S]*?\*\//g],
  rust: [/\/\/.*$/gm, /\/\*[\s\S]*?\*\//g],
  css: [/\/\*[\s\S]*?\*\//g],
  html: [/<!--[\s\S]*?-->/g],
  sh: [/#.*$/gm],
  yaml: [/#.*$/gm],
  yml: [/#.*$/gm],
};

// 임계값 설정
const COMMENT_RATIO_THRESHOLD = 0.3; // 30% 이상이면 경고
const MIN_LINES_TO_CHECK = 10; // 최소 10줄 이상일 때만 체크

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const result = checkComments(data);
    console.log(JSON.stringify(result));
    process.exit(0);
  } catch (err) {
    console.error('Error:', err.message);
    process.exit(0);
  }
});

function checkComments(data) {
  const { tool_name, tool_input, tool_response } = data;

  // Edit 또는 Write 도구만 처리
  if (!['Edit', 'Write'].includes(tool_name)) {
    return {};
  }

  // 파일 경로 추출
  const filePath = tool_input.file_path || tool_input.path;
  if (!filePath) {
    return {};
  }

  // 파일 확장자로 언어 감지
  const ext = path.extname(filePath).slice(1).toLowerCase();
  const patterns = COMMENT_PATTERNS[ext];

  if (!patterns) {
    return {};
  }

  // 새로 작성된 코드 추출
  let newCode = '';
  if (tool_name === 'Write') {
    newCode = tool_input.content || '';
  } else if (tool_name === 'Edit') {
    newCode = tool_input.new_string || '';
  }

  if (!newCode || newCode.split('\n').length < MIN_LINES_TO_CHECK) {
    return {};
  }

  // 주석 비율 계산
  const analysis = analyzeComments(newCode, patterns);

  if (analysis.ratio > COMMENT_RATIO_THRESHOLD) {
    return {
      additionalContext: `[COMMENT-CHECKER WARNING]
파일: ${filePath}
주석 비율: ${(analysis.ratio * 100).toFixed(1)}% (${analysis.commentLines}/${analysis.totalLines} lines)

주석이 너무 많습니다. 코드는 자체적으로 설명적이어야 합니다.
- 불필요한 주석 제거
- 복잡한 로직에만 주석 유지
- "what" 보다 "why"를 설명하는 주석 작성

Agent-generated code should be indistinguishable from human code.`
    };
  }

  return {};
}

function analyzeComments(code, patterns) {
  const lines = code.split('\n');
  const totalLines = lines.filter(l => l.trim().length > 0).length;

  let codeWithoutComments = code;
  for (const pattern of patterns) {
    codeWithoutComments = codeWithoutComments.replace(pattern, '');
  }

  const linesWithoutComments = codeWithoutComments
    .split('\n')
    .filter(l => l.trim().length > 0).length;

  const commentLines = totalLines - linesWithoutComments;
  const ratio = totalLines > 0 ? commentLines / totalLines : 0;

  return { totalLines, commentLines, ratio };
}
