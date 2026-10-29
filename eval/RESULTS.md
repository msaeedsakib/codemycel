# Results

The benchmark as run on 2026-10-29: Claude Code 2.1.311, Sonnet 5.5 answering, Opus 5.5 grading, `--bare`, three runs per task and mode. Tokens, cost and tool calls are medians over the runs; correctness and completeness are means. "Graph" calls are calls to the mode's MCP tools. This introduction is written by hand; everything below the tool versions is the unedited output of `report`.

Tool versions: codemycel 0.1.0, GitNexus 1.6.12, Serena 2.1.0, CodeGraph 1.6.2.

| Repository | Task | Kind | Mode | Tokens | Uncached | Cost $ | Correctness | Completeness | Graph · Bash · Read · Edit calls | Result chars | Runs (graded) |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| hono | compose-impact | impact | without | 11830 | 4963 | 0.019 | 93 | 100 | 0 · 2 · 0 · 0 | 5464 | 3 (3) |
| hono | compose-impact | impact | codemycel | 13457 | 2104 | 0.010 | 89 | 100 | 1 · 0 · 0 · 0 | 3378 | 3 (3) |
| hono | compose-impact | impact | gitnexus | 30109 | 3587 | 0.013 | 59 | 43 | 1 · 0 · 0 · 0 | 4562 | 3 (3) |
| hono | compose-impact | impact | serena | 11882 | 3993 | 0.018 | 95 | 100 | 0 · 3 · 0 · 0 | 2015 | 3 (3) |
| hono | compose-impact | impact | codegraph | 12152 | 4097 | 0.016 | 96 | 100 | 0 · 2 · 0 · 0 | 3230 | 3 (3) |
| hono | cookie-orientation | orientation | without | 9506 | 3563 | 0.014 | 100 | 100 | 0 · 2 · 0 · 0 | 2356 | 3 (3) |
| hono | cookie-orientation | orientation | codemycel | 12316 | 2794 | 0.010 | 100 | 100 | 0 · 1 · 0 · 0 | 1541 | 3 (3) |
| hono | cookie-orientation | orientation | gitnexus | 49475 | 6592 | 0.025 | 99 | 100 | 1 · 2 · 0 · 0 | 9455 | 3 (3) |
| hono | cookie-orientation | orientation | serena | 12037 | 4012 | 0.015 | 100 | 100 | 0 · 2 · 0 · 0 | 2775 | 3 (3) |
| hono | cookie-orientation | orientation | codegraph | 8005 | 2986 | 0.011 | 100 | 100 | 0 · 1 · 0 · 0 | 1605 | 3 (3) |
| hono | cookie-parse-edit | edit | without | 33549 | 11350 | 0.039 | 100 | 100 | 0 · 3 · 2 · 4 | 15855 | 3 (3) |
| hono | cookie-parse-edit | edit | codemycel | 27921 | 5488 | 0.024 | 100 | 100 | 1 · 2 · 0 · 4 | 3399 | 3 (3) |
| hono | cookie-parse-edit | edit | gitnexus | 69108 | 8550 | 0.039 | 100 | 100 | 1 · 2 · 0 · 4 | 10711 | 3 (3) |
| hono | cookie-parse-edit | edit | serena | 17913 | 5583 | 0.024 | 100 | 100 | 0 · 3 · 0 · 4 | 2891 | 3 (3) |
| hono | cookie-parse-edit | edit | codegraph | 16850 | 4853 | 0.021 | 100 | 100 | 0 · 2 · 0 · 4 | 2210 | 3 (3) |
| hono | http-exception | impact | without | 13669 | 6036 | 0.024 | 95 | 98 | 0 · 3 · 0 · 0 | 6995 | 3 (3) |
| hono | http-exception | impact | codemycel | 21695 | 5290 | 0.022 | 91 | 100 | 1 · 2 · 0 · 0 | 5599 | 3 (3) |
| hono | http-exception | impact | gitnexus | 59364 | 11527 | 0.041 | 95 | 98 | 1 · 2 · 0 · 0 | 22959 | 3 (3) |
| hono | http-exception | impact | serena | 15575 | 5770 | 0.023 | 91 | 100 | 0 · 2 · 0 · 0 | 6481 | 3 (3) |
| hono | http-exception | impact | codegraph | 21485 | 6185 | 0.024 | 94 | 100 | 0 · 2 · 0 · 0 | 7525 | 3 (3) |
| hono | request-dispatch | orientation | without | 11974 | 5665 | 0.022 | 99 | 89 | 0 · 3 · 0 · 0 | 6327 | 3 (3) |
| hono | request-dispatch | orientation | codemycel | 21464 | 6177 | 0.023 | 98 | 89 | 1 · 2 · 0 · 0 | 6179 | 3 (3) |
| hono | request-dispatch | orientation | gitnexus | 52189 | 8121 | 0.031 | 98 | 93 | 1 · 2 · 0 · 0 | 12968 | 3 (3) |
| hono | request-dispatch | orientation | serena | 13914 | 5397 | 0.020 | 100 | 89 | 0 · 2 · 0 · 0 | 6211 | 3 (3) |
| hono | request-dispatch | orientation | codegraph | 15220 | 8354 | 0.028 | 89 | 89 | 1 · 0 · 0 · 0 | 14068 | 3 (3) |
| hono | router-match-impact | impact | without | 12969 | 5625 | 0.020 | 100 | 100 | 0 · 2 · 0 · 0 | 5904 | 3 (3) |
| hono | router-match-impact | impact | codemycel | 15275 | 5085 | 0.019 | 90 | 92 | 2 · 0 · 0 · 0 | 6676 | 3 (3) |
| hono | router-match-impact | impact | gitnexus | 56193 | 9435 | 0.031 | 94 | 89 | 3 · 1 · 0 · 0 | 23343 | 3 (3) |
| hono | router-match-impact | impact | serena | 17104 | 6724 | 0.025 | 99 | 94 | 0 · 3 · 0 · 0 | 8333 | 3 (3) |
| hono | router-match-impact | impact | codegraph | 20500 | 7814 | 0.026 | 93 | 89 | 1 · 1 · 0 · 0 | 12703 | 3 (3) |
| irixmail | batch-conditional-impact | impact | without | 16037 | 6721 | 0.025 | 96 | 83 | 0 · 4 · 0 · 0 | 8306 | 3 (3) |
| irixmail | batch-conditional-impact | impact | codemycel | 18158 | 2641 | 0.010 | 94 | 75 | 1 · 1 · 0 · 0 | 1710 | 3 (3) |
| irixmail | batch-conditional-impact | impact | gitnexus | 49380 | 4642 | 0.023 | 88 | 75 | 2 · 2 · 0 · 0 | 9330 | 3 (3) |
| irixmail | batch-conditional-impact | impact | serena | 17052 | 5811 | 0.022 | 97 | 75 | 0 · 4 · 0 · 0 | 5323 | 3 (3) |
| irixmail | batch-conditional-impact | impact | codegraph | 33360 | 14604 | 0.044 | 92 | 92 | 1 · 2 · 0 · 0 | 29954 | 3 (3) |
| irixmail | caldav-orientation | orientation | without | 23445 | 7461 | 0.026 | 96 | 100 | 0 · 4 · 0 · 0 | 10032 | 3 (3) |
| irixmail | caldav-orientation | orientation | codemycel | 39037 | 9542 | 0.033 | 93 | 92 | 3 · 1 · 0 · 0 | 18519 | 3 (3) |
| irixmail | caldav-orientation | orientation | gitnexus | 71634 | 9787 | 0.037 | 100 | 100 | 1 · 3 · 0 · 0 | 15807 | 3 (3) |
| irixmail | caldav-orientation | orientation | serena | 20845 | 6586 | 0.023 | 93 | 100 | 0 · 3 · 0 · 0 | 9206 | 3 (3) |
| irixmail | caldav-orientation | orientation | codegraph | 48539 | 4831 | 0.022 | 99 | 100 | 1 · 2 · 0 · 0 | 31885 | 3 (3) |
| irixmail | dkim-orientation | orientation | without | 32205 | 10397 | 0.037 | 100 | 100 | 0 · 5 · 0 · 0 | 15075 | 3 (3) |
| irixmail | dkim-orientation | orientation | codemycel | 36896 | 10036 | 0.036 | 92 | 100 | 2 · 3 · 0 · 0 | 14201 | 3 (3) |
| irixmail | dkim-orientation | orientation | gitnexus | 98306 | 12515 | 0.048 | 85 | 100 | 1 · 4 · 0 · 0 | 20178 | 3 (3) |
| irixmail | dkim-orientation | orientation | serena | 34475 | 10315 | 0.037 | 90 | 100 | 0 · 4 · 0 · 0 | 15421 | 3 (3) |
| irixmail | dkim-orientation | orientation | codegraph | 49679 | 15936 | 0.049 | 100 | 100 | 1 · 2 · 0 · 0 | 31553 | 3 (3) |
| irixmail | enqueue-edit | edit | without | 57030 | 18401 | 0.073 | 67 | 67 | 0 · 2 · 6 · 8 | 22610 | 3 (3) |
| irixmail | enqueue-edit | edit | codemycel | 69745 | 13352 | 0.059 | 100 | 100 | 1 · 3 · 1 · 8 | 12927 | 3 (3) |
| irixmail | enqueue-edit | edit | gitnexus | 129269 | 19755 | 0.082 | 92 | 95 | 1 · 4 · 1 · 8 | 18066 | 3 (3) |
| irixmail | enqueue-edit | edit | serena | 77931 | 18807 | 0.080 | 100 | 100 | 0 · 2 · 5 · 8 | 21556 | 3 (3) |
| irixmail | enqueue-edit | edit | codegraph | 64478 | 14668 | 0.065 | 100 | 100 | 0 · 4 · 4 · 8 | 15418 | 3 (3) |
| irixmail | enqueue-impact | impact | without | 24154 | 9330 | 0.035 | 91 | 90 | 0 · 2 · 2 · 0 | 10967 | 3 (3) |
| irixmail | enqueue-impact | impact | codemycel | 23747 | 4907 | 0.018 | 84 | 86 | 2 · 0 · 0 · 0 | 9407 | 3 (3) |
| irixmail | enqueue-impact | impact | gitnexus | 44541 | 17677 | 0.053 | 66 | 86 | 1 · 1 · 0 · 0 | 39589 | 3 (3) |
| irixmail | enqueue-impact | impact | serena | 24521 | 7891 | 0.030 | 78 | 86 | 0 · 2 · 2 · 0 | 9034 | 3 (3) |
| irixmail | enqueue-impact | impact | codegraph | 31433 | 16452 | 0.051 | 80 | 86 | 1 · 2 · 0 · 0 | 30867 | 3 (3) |
| irixmail | smtp-to-store-trace | orientation | without | 76918 | 15850 | 0.058 | 94 | 100 | 0 · 6 · 0 · 0 | 30268 | 3 (3) |
| irixmail | smtp-to-store-trace | orientation | codemycel | 116180 | 16897 | 0.067 | 94 | 100 | 2 · 7 · 0 · 0 | 26703 | 3 (3) |
| irixmail | smtp-to-store-trace | orientation | gitnexus | 144194 | 15143 | 0.062 | 100 | 100 | 1 · 5 · 0 · 0 | 27132 | 3 (3) |
| irixmail | smtp-to-store-trace | orientation | serena | 84892 | 19722 | 0.071 | 92 | 100 | 0 · 7 · 0 · 0 | 31476 | 3 (3) |
| irixmail | smtp-to-store-trace | orientation | codegraph | 102766 | 12615 | 0.052 | 96 | 100 | 1 · 4 · 0 · 0 | 45685 | 3 (3) |
| nest | http-route-orientation | orientation | without | 21134 | 6942 | 0.029 | 98 | 96 | 0 · 4 · 0 · 0 | 8085 | 3 (3) |
| nest | http-route-orientation | orientation | codemycel | 29250 | 9612 | 0.033 | 93 | 89 | 2 · 1 · 0 · 0 | 14619 | 3 (3) |
| nest | http-route-orientation | orientation | gitnexus | 34599 | 8796 | 0.032 | 83 | 81 | 1 · 1 · 0 · 0 | 13865 | 3 (3) |
| nest | http-route-orientation | orientation | serena | 18485 | 5867 | 0.023 | 91 | 89 | 0 · 3 · 0 · 0 | 6494 | 3 (3) |
| nest | http-route-orientation | orientation | codegraph | 32377 | 13558 | 0.042 | 99 | 96 | 1 · 0 · 0 · 0 | 27766 | 3 (3) |
| nest | pipe-transform-impact | impact | without | 11889 | 5497 | 0.024 | 66 | 80 | 0 · 3 · 0 · 0 | 4488 | 3 (3) |
| nest | pipe-transform-impact | impact | codemycel | 32928 | 6328 | 0.027 | 70 | 87 | 1 · 2 · 0 · 0 | 8603 | 3 (3) |
| nest | pipe-transform-impact | impact | gitnexus | 50520 | 7773 | 0.033 | 74 | 81 | 1 · 1 · 0 · 0 | 9777 | 3 (3) |
| nest | pipe-transform-impact | impact | serena | 22442 | 7568 | 0.033 | 77 | 81 | 0 · 3 · 0 · 0 | 7866 | 3 (3) |
| nest | pipe-transform-impact | impact | codegraph | 19426 | 14409 | 0.045 | 68 | 85 | 1 · 1 · 0 · 0 | 26843 | 3 (3) |
| nest | tcp-message-trace | orientation | without | 31581 | 10955 | 0.041 | 96 | 100 | 0 · 5 · 0 · 0 | 16949 | 3 (3) |
| nest | tcp-message-trace | orientation | codemycel | 49029 | 10976 | 0.042 | 91 | 100 | 2 · 3 · 0 · 0 | 17567 | 3 (3) |
| nest | tcp-message-trace | orientation | gitnexus | 95152 | 11965 | 0.051 | 95 | 100 | 1 · 3 · 0 · 0 | 18711 | 3 (3) |
| nest | tcp-message-trace | orientation | serena | 38271 | 13822 | 0.049 | 95 | 100 | 0 · 5 · 0 · 0 | 24282 | 3 (3) |
| nest | tcp-message-trace | orientation | codegraph | 18501 | 13494 | 0.042 | 91 | 100 | 1 · 0 · 0 · 0 | 26253 | 3 (3) |
| rust-analyzer | did-change-trace | orientation | without | 24085 | 9635 | 0.035 | 96 | 100 | 0 · 4 · 0 · 0 | 14511 | 3 (3) |
| rust-analyzer | did-change-trace | orientation | codemycel | 24633 | 7067 | 0.027 | 95 | 93 | 2 · 1 · 0 · 0 | 8609 | 3 (3) |
| rust-analyzer | did-change-trace | orientation | gitnexus | 77546 | 11167 | 0.042 | 100 | 96 | 1 · 2 · 0 · 0 | 18942 | 3 (3) |
| rust-analyzer | did-change-trace | orientation | serena | 37297 | 10866 | 0.041 | 99 | 96 | 0 · 5 · 0 · 0 | 16866 | 3 (3) |
| rust-analyzer | did-change-trace | orientation | codegraph | 32686 | 14319 | 0.045 | 93 | 100 | 1 · 1 · 0 · 0 | 27983 | 3 (3) |
| rust-analyzer | hover-orientation | orientation | without | 17406 | 6663 | 0.024 | 99 | 100 | 0 · 4 · 0 · 0 | 8263 | 3 (3) |
| rust-analyzer | hover-orientation | orientation | codemycel | 30082 | 6600 | 0.026 | 82 | 92 | 1 · 2 · 0 · 0 | 7349 | 3 (3) |
| rust-analyzer | hover-orientation | orientation | gitnexus | 51460 | 6206 | 0.027 | 91 | 100 | 1 · 2 · 0 · 0 | 7210 | 3 (3) |
| rust-analyzer | hover-orientation | orientation | serena | 20998 | 6546 | 0.024 | 94 | 100 | 0 · 3 · 0 · 0 | 7597 | 3 (3) |
| rust-analyzer | hover-orientation | orientation | codegraph | 18358 | 2169 | 0.011 | 97 | 96 | 1 · 0 · 0 · 0 | 26140 | 3 (3) |
| rust-analyzer | proc-macro-expander-impact | impact | without | 16795 | 5983 | 0.024 | 93 | 67 | 0 · 3 · 0 · 0 | 6108 | 3 (3) |
| rust-analyzer | proc-macro-expander-impact | impact | codemycel | 14561 | 4887 | 0.018 | 98 | 100 | 2 · 0 · 0 · 0 | 4830 | 3 (3) |
| rust-analyzer | proc-macro-expander-impact | impact | gitnexus | 53165 | 6737 | 0.029 | 90 | 71 | 1 · 2 · 0 · 0 | 8253 | 3 (3) |
| rust-analyzer | proc-macro-expander-impact | impact | serena | 17366 | 6919 | 0.028 | 91 | 67 | 0 · 3 · 0 · 0 | 7836 | 3 (3) |
| rust-analyzer | proc-macro-expander-impact | impact | codegraph | 32864 | 3755 | 0.020 | 78 | 79 | 1 · 1 · 0 · 0 | 27551 | 3 (3) |

| Mode | Tasks | Total median tokens | Uncached | Cost $ | Mean correctness | Mean completeness |
|---|---:|---:|---:|---:|---:|---:|
| without | 18 | 446176 | 151037 | 0.568 | 93 | 93 |
| codemycel | 18 | 596374 | 129783 | 0.503 | 92 | 94 |
| gitnexus | 18 | 1216204 | 179975 | 0.700 | 89 | 89 |
| serena | 18 | 503000 | 152199 | 0.585 | 93 | 93 |
| codegraph | 18 | 578679 | 175099 | 0.613 | 92 | 95 |

| Kind | Mode | Tasks | Total median tokens | Mean correctness | Mean completeness |
|---|---|---:|---:|---:|---:|
| edit | without | 2 | 90579 | 83 | 83 |
| edit | codemycel | 2 | 97666 | 100 | 100 |
| edit | gitnexus | 2 | 198377 | 96 | 98 |
| edit | serena | 2 | 95844 | 100 | 100 |
| edit | codegraph | 2 | 81328 | 100 | 100 |
| impact | without | 7 | 107343 | 90 | 88 |
| impact | codemycel | 7 | 139821 | 88 | 91 |
| impact | gitnexus | 7 | 343272 | 81 | 78 |
| impact | serena | 7 | 125942 | 90 | 86 |
| impact | codegraph | 7 | 171220 | 86 | 90 |
| orientation | without | 9 | 248254 | 97 | 98 |
| orientation | codemycel | 9 | 358887 | 93 | 95 |
| orientation | gitnexus | 9 | 674555 | 95 | 97 |
| orientation | serena | 9 | 281214 | 95 | 97 |
| orientation | codegraph | 9 | 326131 | 96 | 98 |

| codemycel against | Tasks won 2× (correctness ≥, completeness ≥, tokens ≤ ½) | Near misses (within 3 points) | Overall 2× on the shared tasks |
|---|---:|---:|---|
| gitnexus | 6 of 18 | 0 | yes |
| serena | 0 of 18 | 0 | no |
| codegraph | 1 of 18 | 0 | no |
