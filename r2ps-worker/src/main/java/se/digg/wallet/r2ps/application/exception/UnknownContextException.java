// SPDX-FileCopyrightText: 2025 diggsweden/wallet-r2ps
//
// SPDX-License-Identifier: EUPL-1.2

package se.digg.wallet.r2ps.application.exception;

/**
 * Exception thrown when a context is unknown for this worker. Probable cause is misconfiguration
 * and the request should not have been sent to this worker.
 */
public class UnknownContextException extends RuntimeException {

}
