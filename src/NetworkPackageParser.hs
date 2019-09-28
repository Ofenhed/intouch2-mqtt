module NetworkPackageParser where

import NetworkPackage

import Control.Applicative
import Text.ParserCombinators.ReadP as R

import qualified Data.ByteString.Char8 as C8

parsePackage :: ReadP [NetworkPackage]
parsePackage = do
  result <- R.many (parseAuthorizedPackage <++ parseHelloPackage)
  eof
  return result

parseHelloPackage :: ReadP NetworkPackage
parseHelloPackage = do
  string "<HELLO>"
  s <- R.many get
  string "</HELLO>"
  return $ HelloPackage $ C8.pack s

parseAuthorizedPackage :: ReadP NetworkPackage
parseAuthorizedPackage = do
  string "<PACKT>"
  sender <- option Nothing $ do
    string "<SRCCN>"
    s <- R.many get
    string "</SRCCN>"
    return $ Just $ C8.pack s

  destination <- option Nothing $ do
    string "<DESCN>"
    d <- R.many get
    string "</DESCN>"
    return $ Just $ C8.pack d
  string "<DATAS>"
  d <- R.many get
  string "</DATAS>"
  string "</PACKT>"
  return $ AuthorizedNetworkPackage sender destination $ case d of
                                                           "APING" -> NetPing
                                                           "APING." -> NetPong
                                                           d' -> NetUnknownPackage (C8.pack d')
