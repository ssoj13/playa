#!/usr/bin/env node

/**
 * todo-continuation.js
 *
 * Stop 훅: TODO 미완료시 자동으로 작업 계속
 *
 * 설정 방법 (settings.json):
 * {
 *   "hooks": {
 *     "Stop": [{
 *       "matcher": "*",
 *       "hooks": [{
 *         "type": "command",
 *         "command": "node ~/.claude/hooks/todo-continuation.js"
 *       }]
 *     }]
 *   }
 * }
 */

import fs from 'fs';

// stdin에서 입력 읽기
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const result = checkTodoAndContinue(data);
    console.log(JSON.stringify(result));
    process.exit(0);
  } catch (err) {
    console.error('Error:', err.message);
    process.exit(0);
  }
});

function checkTodoAndContinue(data) {
  const { session_id, cwd, transcript_path } = data;

  // transcript에서 TODO 상태 확인
  const todos = extractTodosFromTranscript(transcript_path);

  if (!todos || todos.length === 0) {
    return {};
  }

  // 미완료 TODO 확인
  const pendingTodos = todos.filter(t =>
    t.status === 'pending' || t.status === 'in_progress'
  );

  if (pendingTodos.length === 0) {
    return {};
  }

  // 미완료 항목이 있으면 계속 진행하도록
  const todoList = pendingTodos
    .map(t => `- [${t.status}] ${t.content}`)
    .join('\n');

  return {
    continue: true,
    stopReason: `[AUTO-CONTINUE] 아직 완료되지 않은 TODO가 있습니다:\n\n${todoList}\n\n위 TODO 항목들을 계속 진행해주세요.`
  };
}

function extractTodosFromTranscript(transcriptPath) {
  if (!transcriptPath || !fs.existsSync(transcriptPath)) {
    return null;
  }

  try {
    const content = fs.readFileSync(transcriptPath, 'utf8');

    // TodoWrite 도구 호출에서 TODO 추출
    const todoMatches = content.match(/TodoWrite.*?"todos":\s*(\[[\s\S]*?\])/g);

    if (!todoMatches || todoMatches.length === 0) {
      return null;
    }

    // 마지막 TODO 상태 가져오기
    const lastMatch = todoMatches[todoMatches.length - 1];
    const jsonMatch = lastMatch.match(/\[[\s\S]*?\]/);

    if (jsonMatch) {
      return JSON.parse(jsonMatch[0]);
    }
  } catch (err) {
    // 파싱 실패시 무시
  }

  return null;
}
